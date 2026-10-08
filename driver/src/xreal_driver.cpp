// Prototype SteamVR driver for XREAL glasses: a headset that uses SteamVR's *direct mode* so the
// compositor does not need a DRM lease of the display. This first version only proves the plumbing:
// it advertises an HMD, lets SteamVR allocate swap textures (dma-bufs), counts the frames SteamVR
// presents and logs them. It does not display anything yet and reports a fixed head pose.
//
// How the Linux direct-mode API is used here follows ALVR's driver (MIT licensed),
// alvr/server_openvr/cpp/platform/linux/OvrDirectModeComponent.cpp, which is the reference for a
// working implementation.
#include "openvr_driver.h"

#include <poll.h>
#include <sys/socket.h>
#include <sys/types.h>
#include <sys/uio.h>
#include <sys/un.h>
#include <unistd.h>

#include <stddef.h>

#include <cerrno>
#include <math.h>
#include <atomic>
#include <chrono>
#include <condition_variable>
#include <cstdarg>
#include <cstdio>
#include <cstring>
#include <deque>
#include <map>
#include <memory>
#include <mutex>
#include <string>
#include <thread>
#include <vector>

#define XREAL_EXPORT extern "C" __attribute__((visibility("default")))

static void Log(const char *fmt, ...) {
    char buf[1024];
    va_list args;
    va_start(args, fmt);
    vsnprintf(buf, sizeof(buf), fmt, args);
    va_end(args);
    vr::VRDriverLog()->Log(buf);
}

// CLOCK_MONOTONIC in libstdc++, the same clock the presenter stamps its vblank and pose times with.
static int64_t NowNs() {
    return std::chrono::duration_cast<std::chrono::nanoseconds>(std::chrono::steady_clock::now().time_since_epoch()).count();
}

// Valve's "running start": vsync is declared this far ahead of the real vblank so SteamVR starts each frame with headroom.
static const int64_t kRunningStartNs = 8'000'000;   // 2 ms let SteamVR's bad frames through with Home on; 8 ms and more did not (docs/findings.md)

// Vulkan values we need without including vulkan.h
static const uint32_t kUsageTransferSrc = 0x1, kUsageSampled = 0x4, kUsageInputAttachment = 0x80;
static const uint32_t kCreateMutableFormat = 0x8;
static const uint32_t kUsageFlags = kUsageTransferSrc | kUsageSampled | kUsageInputAttachment;

// ---- link to the presenter process ---------------------------------------------------------------
// The presenter (see presenter/) owns the glasses' display. This driver sends it each swap-texture set's file
// descriptors when SteamVR creates them, and a small message for every presented frame. Messages are fixed arrays of
// 16 u32 words over a SOCK_SEQPACKET unix socket; file descriptors travel as SCM_RIGHTS.
//   type 1 SET:     [1, set_id, width, height, vk_format, usage, create_flags]   + 3 fds
//   type 2 DESTROY: [2, set_id]
//   type 3 PRESENT: [3, left_set, left_index, right_set, right_index, frame_number]
// and from the presenter:
//   type 4 POSE:    [4, imu_ts_lo, imu_ts_hi, w, x, y, z, valid, wx, wy, wz, host_ns_lo, host_ns_hi]
//   type 5 VBLANK:  [5, sequence, monotonic_ns_lo, monotonic_ns_hi]   (0 ns: the time of arrival is the best estimate)
//   type 6 USING:   [6, frame_number]   it reads that frame from now on and has released every older one
//   type 7 CONFIG:  [7, ipd_m, left, right, top, bottom]   f32 bits; the glasses' own field of view (raw projection tangents) and IPD
enum MsgType : uint32_t { kMsgSet = 1, kMsgDestroy = 2, kMsgPresent = 3, kMsgPose = 4, kMsgVblank = 5, kMsgUsing = 6, kMsgConfig = 7 };

class PresenterLink {
public:
    ~PresenterLink() { Close(); }

    int Fd() const { return fd_; }
    bool IsOpen() const { return fd_ >= 0; }

    // Opens the connection if needed (at most every 500 ms); only the pose thread calls this, to keep syscalls off SteamVR's thread.
    bool Connected() {
        if (fd_ >= 0) return true;
        auto now = std::chrono::steady_clock::now();
        if (now - lastTry_ < std::chrono::milliseconds(500)) return false;
        lastTry_ = now;
        int s = socket(AF_UNIX, SOCK_SEQPACKET | SOCK_CLOEXEC | SOCK_NONBLOCK, 0);
        if (s < 0) return false;
        // An abstract socket (leading NUL) lives in the network namespace, not the filesystem, so it works from
        // inside SteamVR's pressure-vessel container, which cannot see files under /run/user.
        sockaddr_un addr{};
        addr.sun_family = AF_UNIX;
        int n = snprintf(addr.sun_path + 1, sizeof(addr.sun_path) - 1, "xreal-presenter-%u", (unsigned)getuid());
        socklen_t len = offsetof(sockaddr_un, sun_path) + 1 + n;
        if (connect(s, (sockaddr *)&addr, len) != 0) { close(s); return false; }
        fd_ = s;
        justConnected_ = true;
        Log("connected to presenter (abstract socket %s)", addr.sun_path + 1);
        return true;
    }

    // True once after each new connection, so the owner can re-send everything it already created.
    bool TakeJustConnected() { bool j = justConnected_; justConnected_ = false; return j; }

    bool Send(const uint32_t (&words)[16], const int *fds = nullptr, int nfds = 0) {
        if (fd_ < 0) return false;
        iovec iov{(void *)words, sizeof(words)};
        char ctl[CMSG_SPACE(sizeof(int) * 3)] = {};
        msghdr mh{};
        mh.msg_iov = &iov;
        mh.msg_iovlen = 1;
        if (nfds > 0) {
            mh.msg_control = ctl;
            mh.msg_controllen = CMSG_SPACE(sizeof(int) * nfds);
            cmsghdr *c = CMSG_FIRSTHDR(&mh);
            c->cmsg_level = SOL_SOCKET;
            c->cmsg_type = SCM_RIGHTS;
            c->cmsg_len = CMSG_LEN(sizeof(int) * nfds);
            memcpy(CMSG_DATA(c), fds, sizeof(int) * nfds);
        }
        if (sendmsg(fd_, &mh, MSG_NOSIGNAL | MSG_DONTWAIT) != (ssize_t)sizeof(words)) {
            Log("presenter link lost");
            Close();
            return false;
        }
        return true;
    }

    // 1: a message was read into words, 0: nothing waiting, -1: the connection is gone.
    int Recv(uint32_t (&words)[16]) {
        if (fd_ < 0) return -1;
        ssize_t n = recv(fd_, words, sizeof(words), MSG_DONTWAIT);
        if (n == (ssize_t)sizeof(words)) return 1;
        if (n < 0 && (errno == EAGAIN || errno == EWOULDBLOCK)) return 0;
        Log("presenter link closed");
        Close();
        return -1;
    }

    void Close() {
        if (fd_ >= 0) close(fd_);
        fd_ = -1;
    }

private:
    int fd_ = -1;
    bool justConnected_ = false;
    std::chrono::steady_clock::time_point lastTry_{};
};

struct Settings {
    int renderWidth = 1920, renderHeight = 1080;  // per eye, the panel's size; the Deck held 60 fps with Home on at this size once paced (docs/findings.md)
    int windowWidth = 3840, windowHeight = 1080;  // both eyes side by side on the glasses
    float refreshHz = 60.f;
    float ipd = 0.064f;
    float headHeight = 1.5f;  // metres above the floor in SteamVR's standing space
    bool sendAngularVelocity = true;  // lets SteamVR predict ahead; settable for diagnosing glitches
    bool headModel = true;            // lets SteamVR add head/neck translation from rotation
    bool holdAfterPresent = true;     // pace SteamVR in PostPresent; settable to compare with and without
    float holdMaxMs = -1.f;           // longest PostPresent hold; negative means until the next running start
    float runningStartMs = kRunningStartNs / 1e6f;  // how long before the real vblank SteamVR is told the vsync happens, and released from PostPresent
    float vsyncToPhotons = -1.f;      // seconds; negative means the pipeline's known part (see VsyncToPhotons)
    std::string serial = "XREAL-PROTOTYPE-0001";
    std::string model = "XREAL 1S";

    void Load() {
        auto *s = vr::VRSettings();
        auto getInt = [&](const char *key, int &v) {
            vr::EVRSettingsError e;
            int32_t r = s->GetInt32("driver_xreal", key, &e);
            if (e == vr::VRSettingsError_None) v = r;
        };
        getInt("render_width", renderWidth);
        getInt("render_height", renderHeight);
        getInt("window_width", windowWidth);
        getInt("window_height", windowHeight);
        vr::EVRSettingsError e;
        float hz = s->GetFloat("driver_xreal", "display_frequency", &e);
        if (e == vr::VRSettingsError_None && hz > 0) refreshHz = hz;
        float h = s->GetFloat("driver_xreal", "head_height", &e);
        if (e == vr::VRSettingsError_None) headHeight = h;
        bool b = s->GetBool("driver_xreal", "send_angular_velocity", &e);
        if (e == vr::VRSettingsError_None) sendAngularVelocity = b;
        b = s->GetBool("driver_xreal", "head_model", &e);
        if (e == vr::VRSettingsError_None) headModel = b;
        b = s->GetBool("driver_xreal", "hold_after_present", &e);
        if (e == vr::VRSettingsError_None) holdAfterPresent = b;
        float m = s->GetFloat("driver_xreal", "hold_max_ms", &e);
        if (e == vr::VRSettingsError_None) holdMaxMs = m;
        float rs = s->GetFloat("driver_xreal", "running_start_ms", &e);
        if (e == vr::VRSettingsError_None && rs >= 0) runningStartMs = rs;
        float v = s->GetFloat("driver_xreal", "seconds_from_vsync_to_photons", &e);
        if (e == vr::VRSettingsError_None) vsyncToPhotons = v;
    }

    int64_t PeriodNs() const { return (int64_t)(1e9 / refreshHz); }

    // The declared vsync is a running start before the real vblank, and the presenter shows a frame one refresh after the vblank
    // it picks it up at; the panel's own latency is not measured, so it is left out unless set.
    float VsyncToPhotons() const { return vsyncToPhotons >= 0 ? vsyncToPhotons : runningStartMs / 1e3f + 1.f / refreshHz; }
};

// Shared by the vsync announcer and PostPresent.
struct Pacing {
    int64_t periodNs = 0;
    bool holdAfterPresent = true;
    int64_t holdMaxNs = -1;
    int64_t runningStartNs = kRunningStartNs;
    std::atomic<int64_t> lastTickNs{0};  // the real vblank whose vsync was most recently declared to SteamVR
};

// What one poll of the presenter link produced.
struct PresenterInput {
    double q[4] = {1, 0, 0, 0};
    double omega[3] = {0, 0, 0};
    bool tracked = false;
    bool newPose = false;
    int64_t sampleNs = 0;  // when the newest pose was sampled, 0 if unknown
    int64_t vblankNs = 0;  // the newest vblank reported, 0 if none

};

static float BitsToFloat(uint32_t b) { float f; memcpy(&f, &b, 4); return f; }

// Rotation part of a 3x4 pose matrix as a unit quaternion (w, x, y, z).
static void MatToQuat(const vr::HmdMatrix34_t &a, float q[4]) {
    const float (*m)[4] = a.m;
    float t = m[0][0] + m[1][1] + m[2][2];
    if (t > 0) {
        float s = 0.5f / sqrtf(t + 1.0f);
        q[0] = 0.25f / s; q[1] = (m[2][1] - m[1][2]) * s; q[2] = (m[0][2] - m[2][0]) * s; q[3] = (m[1][0] - m[0][1]) * s;
    } else if (m[0][0] > m[1][1] && m[0][0] > m[2][2]) {
        float s = 2.0f * sqrtf(1.0f + m[0][0] - m[1][1] - m[2][2]);
        q[0] = (m[2][1] - m[1][2]) / s; q[1] = 0.25f * s; q[2] = (m[0][1] + m[1][0]) / s; q[3] = (m[0][2] + m[2][0]) / s;
    } else if (m[1][1] > m[2][2]) {
        float s = 2.0f * sqrtf(1.0f + m[1][1] - m[0][0] - m[2][2]);
        q[0] = (m[0][2] - m[2][0]) / s; q[1] = (m[0][1] + m[1][0]) / s; q[2] = 0.25f * s; q[3] = (m[1][2] + m[2][1]) / s;
    } else {
        float s = 2.0f * sqrtf(1.0f + m[2][2] - m[0][0] - m[1][1]);
        q[0] = (m[1][0] - m[0][1]) / s; q[1] = (m[0][2] + m[2][0]) / s; q[2] = (m[1][2] + m[2][1]) / s; q[3] = 0.25f * s;
    }
}

// The per-unit geometry the presenter reads from the glasses; until it arrives the constants below are used.
struct Geometry {
    std::atomic<bool> have{false};
    std::atomic<float> fov[4]{{-0.3857f}, {0.3857f}, {-0.2190f}, {0.2190f}};  // left, right, top, bottom
    std::atomic<float> ipd{0.064f};
};

// ---- display geometry --------------------------------------------------------------------------
class DisplayComponent : public vr::IVRDisplayComponent {
public:
    DisplayComponent(const Settings &s, const Geometry &g) : s_(s), g_(g) {}
    void GetWindowBounds(int32_t *x, int32_t *y, uint32_t *w, uint32_t *h) override {
        *x = 0; *y = 0; *w = s_.windowWidth; *h = s_.windowHeight;
    }
    bool IsDisplayOnDesktop() override { return false; }
    bool IsDisplayRealDisplay() override { return false; }
    void GetRecommendedRenderTargetSize(uint32_t *w, uint32_t *h) override {
        *w = s_.renderWidth; *h = s_.renderHeight;
    }
    void GetEyeOutputViewport(vr::EVREye eye, uint32_t *x, uint32_t *y, uint32_t *w, uint32_t *h) override {
        *y = 0; *w = s_.windowWidth / 2; *h = s_.windowHeight;
        *x = eye == vr::Eye_Left ? 0 : s_.windowWidth / 2;
    }
    void GetProjectionRaw(vr::EVREye, float *l, float *r, float *t, float *b) override {
        *l = g_.fov[0]; *r = g_.fov[1]; *t = g_.fov[2]; *b = g_.fov[3];
    }
    vr::DistortionCoordinates_t ComputeDistortion(vr::EVREye, float u, float v) override {
        vr::DistortionCoordinates_t c{};
        c.rfRed[0] = c.rfGreen[0] = c.rfBlue[0] = u;
        c.rfRed[1] = c.rfGreen[1] = c.rfBlue[1] = v;
        return c;
    }
    bool ComputeInverseDistortion(vr::HmdVector2_t *, vr::EVREye, uint32_t, float, float) override { return false; }

private:
    Settings s_;
    const Geometry &g_;
};

// ---- direct mode: swap textures come from SteamVR as dma-bufs ----------------------------------
class DirectModeComponent : public vr::IVRDriverDirectModeComponent {
public:
    DirectModeComponent(Pacing &pacing, Geometry &geometry) : pacing_(pacing), geometry_(geometry) {}
    ~DirectModeComponent() { DestroyAllSwapTextureSets(0, true); }

    void CreateSwapTextureSet(uint32_t pid, const SwapTextureSetDesc_t *desc, SwapTextureSet_t *out) override {
        Log("CreateSwapTextureSet pid=%u format=%u %ux%u samples=%u", pid, desc->nFormat, desc->nWidth,
            desc->nHeight, desc->nSampleCount);
        auto set = std::make_unique<TextureSet>();
        set->pid = pid;
        set->desc = *desc;
        for (int i = 0; i < 3; i++) {
            vr::SharedTextureHandle_t handle = 0;
            bool ok = vr::VRIPCResourceManager()->NewSharedVulkanImage(
                desc->nFormat, desc->nWidth, desc->nHeight, true, false, true, 1, 1, kCreateMutableFormat,
                kUsageTransferSrc | kUsageSampled | kUsageInputAttachment, &handle);
            if (!ok) { Log("NewSharedVulkanImage failed for image %d", i); Release(*set); return; }
            uint64_t ipc = 0;
            vr::VRIPCResourceManager()->RefResource(handle, &ipc);
            int fd = -1;
            if (!vr::VRIPCResourceManager()->ReceiveSharedFd(ipc, &fd)) {
                Log("ReceiveSharedFd failed for image %d", i);
                vr::VRIPCResourceManager()->UnrefResource(handle);
                Release(*set);
                return;
            }
            set->handles[i] = handle;
            set->fds[i] = fd;
            off_t size = lseek(fd, 0, SEEK_END);
            Log("  texture %d: handle=%llu dma-buf fd=%d size=%lld bytes", i, (unsigned long long)handle, fd,
                (long long)size);
            out->rSharedTextureHandles[i] = handle;
        }
        std::lock_guard<std::mutex> lock(mutex_);
        TextureSet *raw = set.release();
        raw->id = nextSetId_++;
        for (int i = 0; i < 3; i++) byHandle_[raw->handles[i]] = {raw, i};
        sets_.push_back(raw);
        if (link_.IsOpen()) SendSet(*raw);
    }

    void DestroySwapTextureSet(vr::SharedTextureHandle_t handle) override {
        std::lock_guard<std::mutex> lock(mutex_);
        auto it = byHandle_.find(handle);
        if (it == byHandle_.end()) return;
        Log("DestroySwapTextureSet set %u (%ux%u)", it->second.first->id, it->second.first->desc.nWidth, it->second.first->desc.nHeight);
        DestroyLocked(it->second.first);
        released_.notify_all();
    }

    void DestroyAllSwapTextureSets(uint32_t pid) override { DestroyAllSwapTextureSets(pid, false); }

    void GetNextSwapTextureSetIndex(vr::SharedTextureHandle_t handles[2], uint32_t (*indices)[2]) override {
        std::unique_lock<std::mutex> lock(mutex_);
        for (int eye = 0; eye < 2; eye++) (*indices)[eye] = NextFreeIndexLocked(lock, handles[eye], eye);
    }

    void SubmitLayer(const SubmitLayerPerEye_t (&perEye)[2]) override {
        std::lock_guard<std::mutex> lock(mutex_);
        for (int eye = 0; eye < 2; eye++) {
            if (layerCount_ == 0) layer0_[eye] = perEye[eye];
        }
        layerCount_++;
    }

    void Present(vr::SharedTextureHandle_t) override {
        std::lock_guard<std::mutex> lock(mutex_);
        presents_++;
        uint32_t setId[2] = {0, 0}, index[2] = {0, 0};
        for (int eye = 0; eye < 2; eye++) {
            auto it = byHandle_.find(layer0_[eye].hTexture);
            if (it != byHandle_.end()) { setId[eye] = it->second.first->id; index[eye] = it->second.second; }
        }
        if (presents_ <= 3 || presents_ % 300 == 0)
            Log("Present #%u layers=%d left set %u[%u] right set %u[%u]", presents_, layerCount_, setId[0],
                index[0], setId[1], index[1]);
        if (layerCount_ != lastLayerCount_ && layerChangeLogs_ < 300) {   // diagnostic: does the layer count change with UI activity?
            layerChangeLogs_++;
            Log("layers %d -> %d at Present #%u (sets %u/%u)", lastLayerCount_, layerCount_, presents_, setId[0], setId[1]);
        }
        lastLayerCount_ = layerCount_;
        if ((setId[0] == 0 || setId[1] == 0) && unknownTextureLogs_ < 20) {
            unknownTextureLogs_++;
            Log("Present #%u: layer 0 refers to an unknown texture (left %u right %u)", presents_, setId[0], setId[1]);
        }
        if (link_.IsOpen()) {
            ResyncIfNew();
            if (setId[0] && setId[1]) {
                uint32_t m[16] = {kMsgPresent, setId[0], index[0], setId[1], index[1], presents_};
                float rq[4];
                MatToQuat(layer0_[0].mHmdPose, rq);        // the head pose SteamVR rendered this frame for
                for (int i = 0; i < 4; i++) memcpy(&m[6 + i], &rq[i], 4);
                // Valid region of each eye's texture (SteamVR can render into a sub-rectangle when it lowers the resolution).
                // Packed as four u16 (umin, vmin, umax, vmax; 0..65535) per eye in words 10-11 (left) and 12-13 (right).
                bool partial = false;
                for (int eye = 0; eye < 2; eye++) {
                    const vr::VRTextureBounds_t &b = layer0_[eye].bounds;
                    auto pack = [](float v) { v = v < 0 ? 0 : (v > 1 ? 1 : v); return (uint32_t)(v * 65535.0f + 0.5f); };
                    m[10 + 2 * eye] = pack(b.uMin) | (pack(b.vMin) << 16);
                    m[11 + 2 * eye] = pack(b.uMax) | (pack(b.vMax) << 16);
                    if (fabsf(b.uMin) > 0.001f || fabsf(b.vMin) > 0.001f || fabsf(b.uMax - 1) > 0.001f || fabsf(b.vMax - 1) > 0.001f) partial = true;
                }
                if (partial && partialBoundsLogs_ < 200) {
                    partialBoundsLogs_++;
                    const vr::VRTextureBounds_t &l = layer0_[0].bounds, &r = layer0_[1].bounds;
                    Log("Present #%u: PARTIAL bounds left u %.3f-%.3f v %.3f-%.3f right u %.3f-%.3f v %.3f-%.3f", presents_, l.uMin, l.uMax, l.vMin, l.vMax, r.uMin, r.uMax, r.vMin, r.vMax);
                }
                if (link_.Send(m)) {
                    offered_.push_back({presents_, {setId[0], setId[1]}, {index[0], index[1]}});
                    if (offered_.size() > 8) offered_.pop_front();   // a presenter that never sends USING releases nothing
                }
            }
        }
        layerCount_ = 0;
    }

    // Valve's direct-mode guidance: hold SteamVR here until the next running start, so its next frame starts on the display's
    // schedule rather than as soon as this one is presented.
    void PostPresent(const Throttling_t *throttling) override {
        int64_t last = pacing_.lastTickNs.load();
        if (!pacing_.holdAfterPresent || last == 0) return;
        uint32_t extra = throttling ? throttling->nFramesToThrottle : 0;
        int64_t wait = last + pacing_.periodNs * (1 + extra) - pacing_.runningStartNs - NowNs();
        if (pacing_.holdMaxNs >= 0 && wait > pacing_.holdMaxNs) wait = pacing_.holdMaxNs;
        if (wait > 0) std::this_thread::sleep_for(std::chrono::nanoseconds(wait));
    }

    // Wait up to timeoutMs for the presenter, then read everything it sent.
    void PollMessages(PresenterInput &in, int timeoutMs) {
        int fd;
        { std::lock_guard<std::mutex> lock(mutex_); fd = link_.Connected() ? link_.Fd() : -1; }
        if (fd < 0) { std::this_thread::sleep_for(std::chrono::milliseconds(timeoutMs)); return; }
        pollfd pfd{fd, POLLIN, 0};
        poll(&pfd, 1, timeoutMs);
        std::lock_guard<std::mutex> lock(mutex_);
        if (!link_.IsOpen()) return;
        ResyncIfNew();
        uint32_t w[16];
        int r;
        while ((r = link_.Recv(w)) > 0) {
            if (w[0] == kMsgPose) {
                for (int i = 0; i < 4; i++) pose_[i] = BitsToFloat(w[3 + i]);
                for (int i = 0; i < 3; i++) omega_[i] = BitsToFloat(w[8 + i]);
                poseValid_ = w[7] != 0;
                in.sampleNs = (int64_t)((uint64_t)w[11] | ((uint64_t)w[12] << 32));
                in.newPose = true;
            } else if (w[0] == kMsgVblank) {
                int64_t t = (int64_t)((uint64_t)w[2] | ((uint64_t)w[3] << 32));
                in.vblankNs = t ? t : NowNs();
            } else if (w[0] == kMsgConfig) {
                float fov[4] = {BitsToFloat(w[2]), BitsToFloat(w[3]), BitsToFloat(w[4]), BitsToFloat(w[5])};
                float ipd = BitsToFloat(w[1]);
                if (fov[0] < 0 && fov[1] > 0 && fov[2] < 0 && fov[3] > 0 && ipd > 0.04f && ipd < 0.09f) {
                    for (int i = 0; i < 4; i++) geometry_.fov[i] = fov[i];
                    geometry_.ipd = ipd;
                    geometry_.have = true;
                }
            } else if (w[0] == kMsgUsing) {
                presenterReleases_ = true;
                while (!offered_.empty() && (int32_t)(offered_.front().frame - w[1]) < 0) offered_.pop_front();
                released_.notify_all();
            }
        }
        if (r < 0) ForgetPresenterLocked();
        in.tracked = poseValid_;
        for (int i = 0; i < 4; i++) in.q[i] = pose_[i];
        for (int i = 0; i < 3; i++) in.omega[i] = omega_[i];
    }

    void GetFrameTiming(vr::DriverDirectMode_FrameTiming *t) override {
        if (t->m_nSize < sizeof(vr::DriverDirectMode_FrameTiming)) return;
        t->m_nNumFramePresents = 1;   // times *this* frame was shown, not a running total
        t->m_nNumMisPresented = 0;
        t->m_nNumDroppedFrames = 0;
        t->m_nReprojectionFlags = 0;
    }

private:
    struct TextureSet {
        uint32_t id = 0;
        uint32_t pid = 0;
        SwapTextureSetDesc_t desc{};
        vr::SharedTextureHandle_t handles[3] = {0, 0, 0};
        int fds[3] = {-1, -1, -1};
        uint32_t next = 0;
    };

    // A frame sent to the presenter, which may read it until it reports using a newer one.
    struct Offered {
        uint32_t frame;
        uint32_t set[2];
        uint32_t index[2];
    };

    static void Release(TextureSet &set) {
        for (int i = 0; i < 3; i++) {
            if (set.fds[i] >= 0) { close(set.fds[i]); set.fds[i] = -1; }
            if (set.handles[i]) { vr::VRIPCResourceManager()->UnrefResource(set.handles[i]); set.handles[i] = 0; }
        }
    }

    bool HeldLocked(uint32_t setId, uint32_t index, int eye) const {
        if (!presenterReleases_) return false;
        for (const Offered &o : offered_)
            if (o.set[eye] == setId && o.index[eye] == index) return true;
        return false;
    }

    // The next image of the set the presenter is not reading. Waits for a release if all are held, and after about a refresh
    // and a half reclaims the oldest, so a stalled presenter cannot stop SteamVR.
    uint32_t NextFreeIndexLocked(std::unique_lock<std::mutex> &lock, vr::SharedTextureHandle_t handle, int eye) {
        auto timeout = std::chrono::nanoseconds(pacing_.periodNs * 3 / 2);
        for (;;) {
            auto it = byHandle_.find(handle);
            if (it == byHandle_.end()) return 0;
            TextureSet *set = it->second.first;
            for (uint32_t k = 1; k <= 3; k++) {
                uint32_t i = (set->next + k) % 3;
                if (!HeldLocked(set->id, i, eye)) { set->next = i; return i; }
            }
            if (released_.wait_for(lock, timeout) == std::cv_status::timeout && !offered_.empty()) {
                if (reclaimLogs_ < 20) { reclaimLogs_++; Log("presenter held every image of set %u; reclaiming frame %u", set->id, offered_.front().frame); }
                offered_.pop_front();
            }
        }
    }

    void ForgetPresenterLocked() {
        poseValid_ = false;
        offered_.clear();
        presenterReleases_ = false;
        released_.notify_all();
    }

    // A presenter that started later (or restarted) needs every existing set again.
    void ResyncIfNew() {
        if (!link_.TakeJustConnected()) return;
        ForgetPresenterLocked();
        for (auto *s : sets_) SendSet(*s);
    }

    void SendSet(const TextureSet &s) {
        uint32_t m[16] = {kMsgSet, s.id, s.desc.nWidth, s.desc.nHeight, s.desc.nFormat, kUsageFlags, kCreateMutableFormat};
        link_.Send(m, s.fds, 3);
    }

    void DestroyLocked(TextureSet *set) {
        if (link_.IsOpen()) { uint32_t m[16] = {kMsgDestroy, set->id}; link_.Send(m); }
        for (int i = 0; i < 3; i++) byHandle_.erase(set->handles[i]);
        for (size_t i = 0; i < sets_.size(); i++)
            if (sets_[i] == set) { sets_.erase(sets_.begin() + i); break; }
        Release(*set);
        delete set;
    }

    void DestroyAllSwapTextureSets(uint32_t pid, bool all) {
        std::lock_guard<std::mutex> lock(mutex_);
        std::vector<TextureSet *> victims;
        for (auto *s : sets_) if (all || s->pid == pid) victims.push_back(s);
        for (auto *s : victims) DestroyLocked(s);
        released_.notify_all();
    }

    Pacing &pacing_;
    Geometry &geometry_;
    std::mutex mutex_;
    std::condition_variable released_;
    std::vector<TextureSet *> sets_;
    std::map<vr::SharedTextureHandle_t, std::pair<TextureSet *, int>> byHandle_;
    std::deque<Offered> offered_;
    bool presenterReleases_ = false;  // only a presenter that sends USING has images held for it
    SubmitLayerPerEye_t layer0_[2]{};
    int layerCount_ = 0;
    uint32_t presents_ = 0;
    uint32_t nextSetId_ = 1;
    int lastLayerCount_ = -1;
    int layerChangeLogs_ = 0;
    int partialBoundsLogs_ = 0;
    int unknownTextureLogs_ = 0;
    int reclaimLogs_ = 0;
    PresenterLink link_;
    double pose_[4] = {1, 0, 0, 0};
    double omega_[3] = {0, 0, 0};
    bool poseValid_ = false;
};

// ---- the headset ------------------------------------------------------------------------------
class HmdDevice : public vr::ITrackedDeviceServerDriver {
public:
    HmdDevice() {
        settings_.Load();
        pacing_.periodNs = settings_.PeriodNs();
        pacing_.holdAfterPresent = settings_.holdAfterPresent;
        pacing_.runningStartNs = (int64_t)(settings_.runningStartMs * 1e6);
        pacing_.holdMaxNs = settings_.holdMaxMs < 0 ? -1 : (int64_t)(settings_.holdMaxMs * 1e6);
        display_ = std::make_unique<DisplayComponent>(settings_, geometry_);
        direct_ = std::make_unique<DirectModeComponent>(pacing_, geometry_);
    }

    const std::string &Serial() const { return settings_.serial; }

    vr::EVRInitError Activate(uint32_t id) override {
        id_ = id;
        active_ = true;
        // The presenter, started first, sends the glasses' own field of view and IPD as soon as the link is up.
        PresenterInput unused;
        for (int waited = 0; waited < 2000 && !geometry_.have; waited += 50) direct_->PollMessages(unused, 50);
        Log(geometry_.have ? "using the glasses' own calibration: ipd %.1f mm, half tangents %.4f x %.4f" : "no calibration from the presenter; using the default field of view (%.1f mm, %.4f x %.4f)",
            geometry_.ipd * 1000.f, geometry_.fov[1].load(), geometry_.fov[3].load());
        auto c = vr::VRProperties()->TrackedDeviceToPropertyContainer(id);
        auto *p = vr::VRProperties();
        p->SetStringProperty(c, vr::Prop_ModelNumber_String, settings_.model.c_str());
        p->SetStringProperty(c, vr::Prop_ManufacturerName_String, "XREAL");
        p->SetFloatProperty(c, vr::Prop_UserIpdMeters_Float, geometry_.have ? geometry_.ipd.load() : settings_.ipd);
        p->SetFloatProperty(c, vr::Prop_DisplayFrequency_Float, settings_.refreshHz);
        p->SetFloatProperty(c, vr::Prop_UserHeadToEyeDepthMeters_Float, 0.f);
        p->SetFloatProperty(c, vr::Prop_SecondsFromVsyncToPhotons_Float, settings_.VsyncToPhotons());
        p->SetBoolProperty(c, vr::Prop_IsOnDesktop_Bool, false);
        p->SetBoolProperty(c, vr::Prop_HasDisplayComponent_Bool, true);
        p->SetBoolProperty(c, vr::Prop_HasDriverDirectModeComponent_Bool, true);
        p->SetBoolProperty(c, vr::Prop_DriverDirectModeSendsVsyncEvents_Bool, true);
        Log("HMD activated as device %u (%dx%d per eye, %.1f Hz, direct mode; angular velocity %s, head model %s, hold after present %s (max %.1f ms), running start %.1f ms, vsync to photons %.4f s)",
            id, settings_.renderWidth, settings_.renderHeight, settings_.refreshHz, settings_.sendAngularVelocity ? "on" : "off",
            settings_.headModel ? "on" : "off", settings_.holdAfterPresent ? "on" : "off", settings_.holdMaxMs, settings_.runningStartMs, settings_.VsyncToPhotons());
        pose_thread_ = std::thread([this] { PoseLoop(); });
        vsync_thread_ = std::thread([this] { VsyncLoop(); });
        return vr::VRInitError_None;
    }

    void Deactivate() override {
        if (active_.exchange(false)) {
            pose_thread_.join();
            vsync_thread_.join();
        }
        id_ = vr::k_unTrackedDeviceIndexInvalid;
    }

    void EnterStandby() override {}

    void *GetComponent(const char *name) override {
        if (!strcmp(name, vr::IVRDisplayComponent_Version)) return display_.get();
        if (!strcmp(name, vr::IVRDriverDirectModeComponent_Version)) return direct_.get();
        return nullptr;
    }

    void DebugRequest(const char *, char *response, uint32_t size) override {
        if (size >= 1) response[0] = 0;
    }

    // SteamVR may ask for the pose directly instead of using the streamed updates; it must get the latest tracked pose, not an
    // untracked identity one (which showed up as a single frame rendered for the wrong view).
    vr::DriverPose_t GetPose() override {
        std::lock_guard<std::mutex> lock(poseMutex_);
        int n = ++getPoseCalls_;
        if (n <= 5 || n % 1000 == 0) Log("GetPose called by SteamVR (#%d), tracked pose available: %d", n, havePose_ ? 1 : 0);
        return havePose_ ? lastPose_ : MakePose(false, nullptr, nullptr);
    }

private:
    vr::DriverPose_t MakePose(bool tracked, const double *q, const double *omega) {
        vr::DriverPose_t pose = {};
        pose.qWorldFromDriverRotation.w = 1.f;
        pose.qDriverFromHeadRotation.w = 1.f;
        pose.qRotation.w = 1.f;          // identity until the presenter provides a tracked orientation
        pose.vecPosition[1] = settings_.headHeight;
        if (tracked) {
            pose.qRotation.w = q[0]; pose.qRotation.x = q[1]; pose.qRotation.y = q[2]; pose.qRotation.z = q[3];
            if (settings_.sendAngularVelocity) for (int i = 0; i < 3; i++) pose.vecAngularVelocity[i] = omega[i];   // lets SteamVR predict ahead
            pose.shouldApplyHeadModel = settings_.headModel;   // lets SteamVR add the small head/neck translation from rotation
        }
        pose.poseIsValid = true;
        pose.deviceIsConnected = true;
        pose.result = vr::TrackingResult_Running_OK;
        return pose;
    }

    // Reports each new pose from the presenter, stamped with its age so SteamVR predicts from when it was sampled.
    void PoseLoop() {
        int64_t lastUpdate = 0;
        while (active_) {
            PresenterInput in;
            direct_->PollMessages(in, 2);
            if (in.vblankNs) lastVblankNs_ = in.vblankNs;
            int64_t now = NowNs();
            // Without a new sample there is nothing to tell SteamVR, apart from a periodic update that keeps the headset alive.
            if (!in.newPose && now - lastUpdate < 100'000'000) continue;
            vr::DriverPose_t pose = MakePose(in.tracked, in.q, in.omega);
            if (in.tracked && in.sampleNs > 0 && in.sampleNs < now) pose.poseTimeOffset = (in.sampleNs - now) / 1e9;
            if (in.tracked) { std::lock_guard<std::mutex> lock(poseMutex_); lastPose_ = pose; havePose_ = true; }
            vr::VRServerDriverHost()->TrackedDevicePoseUpdated(id_, pose, sizeof(vr::DriverPose_t));
            lastUpdate = now;
        }
    }

    // Declares each vsync to SteamVR a running start before the glasses' real vblank as the presenter reports it, and free-runs
    // at the configured rate while the presenter reports none.
    void VsyncLoop() {
        const int64_t period = pacing_.periodNs;
        int64_t tick = NowNs() + period, lastTick = 0;
        while (active_) {
            int64_t now = NowNs(), vblank = lastVblankNs_;
            if (vblank && now - vblank < 100'000'000) tick = vblank + period;
            while (tick - pacing_.runningStartNs <= now || tick - lastTick < period / 2) tick += period;
            int64_t announce = tick - pacing_.runningStartNs;
            std::this_thread::sleep_for(std::chrono::nanoseconds(announce - now));
            if (!active_) break;
            vr::VRServerDriverHost()->VsyncEvent((announce - NowNs()) / 1e9);
            pacing_.lastTickNs = tick;
            lastTick = tick;
        }
    }

    Settings settings_;
    Geometry geometry_;
    std::unique_ptr<DisplayComponent> display_;
    std::unique_ptr<DirectModeComponent> direct_;
    uint32_t id_ = vr::k_unTrackedDeviceIndexInvalid;
    std::atomic<bool> active_{false};
    Pacing pacing_;
    std::atomic<int64_t> lastVblankNs_{0};
    std::mutex poseMutex_;
    vr::DriverPose_t lastPose_ = {};
    bool havePose_ = false;
    int getPoseCalls_ = 0;
    std::thread pose_thread_, vsync_thread_;
};

// ---- provider ---------------------------------------------------------------------------------
class DeviceProvider : public vr::IServerTrackedDeviceProvider {
public:
    vr::EVRInitError Init(vr::IVRDriverContext *context) override {
        VR_INIT_SERVER_DRIVER_CONTEXT(context);
        Log("xreal driver prototype starting");
        hmd_ = std::make_unique<HmdDevice>();
        if (!vr::VRServerDriverHost()->TrackedDeviceAdded(hmd_->Serial().c_str(), vr::TrackedDeviceClass_HMD, hmd_.get())) {
            Log("TrackedDeviceAdded failed");
            return vr::VRInitError_Driver_Unknown;
        }
        return vr::VRInitError_None;
    }
    void Cleanup() override { hmd_.reset(); }
    const char *const *GetInterfaceVersions() override { return vr::k_InterfaceVersions; }
    void RunFrame() override {
        vr::VREvent_t ev{};
        while (vr::VRServerDriverHost()->PollNextEvent(&ev, sizeof(ev))) {}
    }
    bool ShouldBlockStandbyMode() override { return false; }
    void EnterStandby() override {}
    void LeaveStandby() override {}

private:
    std::unique_ptr<HmdDevice> hmd_;
};

static DeviceProvider g_provider;

XREAL_EXPORT void *HmdDriverFactory(const char *interfaceName, int *returnCode) {
    if (!strcmp(vr::IServerTrackedDeviceProvider_Version, interfaceName)) return &g_provider;
    if (returnCode) *returnCode = vr::VRInitError_Init_InterfaceNotFound;
    return nullptr;
}
