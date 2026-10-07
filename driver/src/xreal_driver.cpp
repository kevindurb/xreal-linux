// Prototype SteamVR driver for XREAL glasses: a headset that uses SteamVR's *direct mode* so the
// compositor does not need a DRM lease of the display. This first version only proves the plumbing:
// it advertises an HMD, lets SteamVR allocate swap textures (dma-bufs), counts the frames SteamVR
// presents and logs them. It does not display anything yet and reports a fixed head pose.
//
// How the Linux direct-mode API is used here follows ALVR's driver (MIT licensed),
// alvr/server_openvr/cpp/platform/linux/OvrDirectModeComponent.cpp, which is the reference for a
// working implementation.
#include "openvr_driver.h"

#include <sys/types.h>
#include <unistd.h>

#include <atomic>
#include <chrono>
#include <cstdarg>
#include <cstdio>
#include <cstring>
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

// Vulkan values we need without including vulkan.h
static const uint32_t kUsageTransferSrc = 0x1, kUsageSampled = 0x4, kUsageInputAttachment = 0x80;
static const uint32_t kCreateMutableFormat = 0x8;

struct Settings {
    int renderWidth = 1920, renderHeight = 1080;  // per eye
    int windowWidth = 3840, windowHeight = 1080;  // both eyes side by side on the glasses
    float refreshHz = 60.f;
    float ipd = 0.063f;
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
    }
};

// ---- display geometry --------------------------------------------------------------------------
class DisplayComponent : public vr::IVRDisplayComponent {
public:
    explicit DisplayComponent(const Settings &s) : s_(s) {}
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
        // Placeholder field of view (about 48 degrees horizontal); to be replaced with the real optics.
        *l = -0.45f; *r = 0.45f; *t = -0.253f; *b = 0.253f;
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
};

// ---- direct mode: swap textures come from SteamVR as dma-bufs ----------------------------------
class DirectModeComponent : public vr::IVRDriverDirectModeComponent {
public:
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
        for (int i = 0; i < 3; i++) byHandle_[raw->handles[i]] = {raw, i};
        sets_.push_back(raw);
    }

    void DestroySwapTextureSet(vr::SharedTextureHandle_t handle) override {
        std::lock_guard<std::mutex> lock(mutex_);
        auto it = byHandle_.find(handle);
        if (it == byHandle_.end()) return;
        DestroyLocked(it->second.first);
    }

    void DestroyAllSwapTextureSets(uint32_t pid) override { DestroyAllSwapTextureSets(pid, false); }

    void GetNextSwapTextureSetIndex(vr::SharedTextureHandle_t handles[2], uint32_t (*indices)[2]) override {
        std::lock_guard<std::mutex> lock(mutex_);
        for (int eye = 0; eye < 2; eye++) {
            auto it = byHandle_.find(handles[eye]);
            if (it == byHandle_.end()) { (*indices)[eye] = 0; continue; }
            TextureSet *set = it->second.first;
            set->next = (set->next + 1) % 3;
            (*indices)[eye] = set->next;
        }
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
        if (presents_ <= 3 || presents_ % 300 == 0) {
            int fds[2] = {-1, -1};
            for (int eye = 0; eye < 2; eye++) {
                auto it = byHandle_.find(layer0_[eye].hTexture);
                if (it != byHandle_.end()) fds[eye] = it->second.first->fds[it->second.second];
            }
            Log("Present #%u layers=%d left fd=%d right fd=%d", presents_, layerCount_, fds[0], fds[1]);
        }
        layerCount_ = 0;
    }

    void PostPresent(const Throttling_t *) override {}

    void GetFrameTiming(vr::DriverDirectMode_FrameTiming *t) override {
        t->m_nSize = sizeof(vr::DriverDirectMode_FrameTiming);
        t->m_nNumFramePresents = presents_;
        t->m_nNumMisPresented = 0;
        t->m_nNumDroppedFrames = 0;
        t->m_nReprojectionFlags = 0;
    }

private:
    struct TextureSet {
        uint32_t pid = 0;
        SwapTextureSetDesc_t desc{};
        vr::SharedTextureHandle_t handles[3] = {0, 0, 0};
        int fds[3] = {-1, -1, -1};
        uint32_t next = 0;
    };

    static void Release(TextureSet &set) {
        for (int i = 0; i < 3; i++) {
            if (set.fds[i] >= 0) { close(set.fds[i]); set.fds[i] = -1; }
            if (set.handles[i]) { vr::VRIPCResourceManager()->UnrefResource(set.handles[i]); set.handles[i] = 0; }
        }
    }

    void DestroyLocked(TextureSet *set) {
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
    }

    std::mutex mutex_;
    std::vector<TextureSet *> sets_;
    std::map<vr::SharedTextureHandle_t, std::pair<TextureSet *, int>> byHandle_;
    SubmitLayerPerEye_t layer0_[2]{};
    int layerCount_ = 0;
    uint32_t presents_ = 0;
};

// ---- the headset ------------------------------------------------------------------------------
class HmdDevice : public vr::ITrackedDeviceServerDriver {
public:
    HmdDevice() {
        settings_.Load();
        display_ = std::make_unique<DisplayComponent>(settings_);
        direct_ = std::make_unique<DirectModeComponent>();
    }

    const std::string &Serial() const { return settings_.serial; }

    vr::EVRInitError Activate(uint32_t id) override {
        id_ = id;
        active_ = true;
        auto c = vr::VRProperties()->TrackedDeviceToPropertyContainer(id);
        auto *p = vr::VRProperties();
        p->SetStringProperty(c, vr::Prop_ModelNumber_String, settings_.model.c_str());
        p->SetStringProperty(c, vr::Prop_ManufacturerName_String, "XREAL");
        p->SetFloatProperty(c, vr::Prop_UserIpdMeters_Float, settings_.ipd);
        p->SetFloatProperty(c, vr::Prop_DisplayFrequency_Float, settings_.refreshHz);
        p->SetFloatProperty(c, vr::Prop_UserHeadToEyeDepthMeters_Float, 0.f);
        p->SetFloatProperty(c, vr::Prop_SecondsFromVsyncToPhotons_Float, 0.011f);
        p->SetBoolProperty(c, vr::Prop_IsOnDesktop_Bool, false);
        p->SetBoolProperty(c, vr::Prop_HasDisplayComponent_Bool, true);
        p->SetBoolProperty(c, vr::Prop_HasDriverDirectModeComponent_Bool, true);
        p->SetBoolProperty(c, vr::Prop_DriverDirectModeSendsVsyncEvents_Bool, true);
        Log("HMD activated as device %u (%dx%d per eye, %.1f Hz, direct mode)", id, settings_.renderWidth,
            settings_.renderHeight, settings_.refreshHz);
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

    vr::DriverPose_t GetPose() override {
        vr::DriverPose_t pose = {};
        pose.qWorldFromDriverRotation.w = 1.f;
        pose.qDriverFromHeadRotation.w = 1.f;
        pose.qRotation.w = 1.f;          // fixed orientation for now; IMU fusion comes later
        pose.poseIsValid = true;
        pose.deviceIsConnected = true;
        pose.result = vr::TrackingResult_Running_OK;
        pose.shouldApplyHeadModel = false;
        return pose;
    }

private:
    void PoseLoop() {
        while (active_) {
            vr::VRServerDriverHost()->TrackedDevicePoseUpdated(id_, GetPose(), sizeof(vr::DriverPose_t));
            std::this_thread::sleep_for(std::chrono::milliseconds(2));
        }
    }

    void VsyncLoop() {
        auto period = std::chrono::duration<double>(1.0 / settings_.refreshHz);
        auto next = std::chrono::steady_clock::now();
        while (active_) {
            next += std::chrono::duration_cast<std::chrono::steady_clock::duration>(period);
            std::this_thread::sleep_until(next);
            vr::VRServerDriverHost()->VsyncEvent(0.0);
        }
    }

    Settings settings_;
    std::unique_ptr<DisplayComponent> display_;
    std::unique_ptr<DirectModeComponent> direct_;
    uint32_t id_ = vr::k_unTrackedDeviceIndexInvalid;
    std::atomic<bool> active_{false};
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
