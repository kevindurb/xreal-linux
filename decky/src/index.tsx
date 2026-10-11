import { ButtonItem, PanelSection, PanelSectionRow, staticClasses } from "@decky/ui";
import { callable, definePlugin, toaster } from "@decky/api";
import { useEffect, useState } from "react";
import { FaGlasses } from "react-icons/fa";

// Backend methods (decky/main.py). Types mirror the dicts the backend returns.
interface UnitState {
  enabled?: string;
  active?: string;
}
interface RuntimeVersion {
  commit: string;
  built: number;
}
interface State {
  user: string;
  units: { socket: UnitState; service: UnitState; headlook: UnitState };
  runtime: { installed: boolean; version: RuntimeVersion | null; path: string };
  report: { data: any; exit: number; error: string | null } | null;
  expected_version: string;
}
interface ModeResult extends State {
  ok?: boolean;
  error?: string;
  needs_confirmation?: boolean;
  reason?: string;
}

const getState = callable<[], State>("get_state");
const setMode = callable<[mode: string, confirm?: boolean], ModeResult>("set_mode");
const installRuntime = callable<[], { ok: boolean; kind?: string; error?: string; tag?: string; output?: string }>("install_runtime");

const MODES: { id: string; label: string; hint: string }[] = [
  { id: "off", label: "Off", hint: "The glasses stay an ordinary monitor; nothing of xreal-linux runs." },
  { id: "head_mouse", label: "Head mouse", hint: "Head motion drives a virtual mouse (no SteamVR)." },
  { id: "steamvr", label: "SteamVR", hint: "The background service waits to present a SteamVR session." },
];

/// The selected mode from the units' real state (enabled wins, so it survives a reboot).
function currentMode(state?: State): string {
  if (!state) return "";
  if (state.units.headlook?.enabled === "enabled") return state.units.socket?.enabled === "enabled" ? "steamvr" : "head_mouse";
  if (state.units.socket?.enabled === "enabled") return "steamvr";
  return "off";
}

function Row({ label, value, good }: { label: string; value: string; good?: boolean }) {
  return (
    <PanelSectionRow>
      <div style={{ display: "flex", justifyContent: "space-between", gap: "8px" }}>
        <span style={{ opacity: 0.8 }}>{label}</span>
        <span style={{ textAlign: "right", color: good === undefined ? undefined : good ? "#7bd17b" : "#e0a030" }}>{value}</span>
      </div>
    </PanelSectionRow>
  );
}

function Content() {
  const [state, setState] = useState<State | undefined>();
  const [busy, setBusy] = useState(false);
  const [pending, setPending] = useState<{ mode: string; reason: string } | null>(null);

  const refresh = async () => {
    try {
      setState(await getState());
    } catch (e) {
      toaster.toast({ title: "XREAL", body: `Could not read the state: ${e}` });
    }
  };

  useEffect(() => {
    refresh();
  }, []);

  const choose = async (mode: string, confirm = false) => {
    setBusy(true);
    setPending(null);
    try {
      const res = await setMode(mode, confirm);
      if (res?.needs_confirmation) {
        setPending({ mode, reason: res.reason ?? "" });
      } else if (res?.ok === false) {
        toaster.toast({ title: "XREAL", body: res.error ?? "The action failed." });
      }
      setState(res);
      await refresh();
    } catch (e) {
      toaster.toast({ title: "XREAL", body: `${e}` });
    } finally {
      setBusy(false);
    }
  };

  const install = async () => {
    setBusy(true);
    try {
      const res = await installRuntime();
      toaster.toast({ title: "XREAL", body: res?.ok ? `Installed the runtime (${res.tag}).` : `Not installed: ${res?.error ?? "unknown error"}` });
      await refresh();
    } finally {
      setBusy(false);
    }
  };

  const mode = currentMode(state);
  const device = state?.report?.data?.device;
  const reachable = !!device?.connected;
  const version = state?.runtime?.version?.commit ?? state?.report?.data?.runtime?.version ?? null;
  const reportUsable = !!state?.report?.data?.device && !!state?.report?.data?.units;

  return (
    <>
      <PanelSection title="Mode">
        {MODES.map((m) => (
          <PanelSectionRow key={m.id}>
            <ButtonItem layout="below" disabled={busy || !reportUsable} onClick={() => choose(m.id)}>
              {`${mode === m.id ? "● " : "○ "}${m.label}`}
            </ButtonItem>
          </PanelSectionRow>
        ))}
        {pending && (
          <>
            <PanelSectionRow>
              <div style={{ color: "#e0a030", fontSize: "0.85em" }}>{pending.reason}</div>
            </PanelSectionRow>
            <PanelSectionRow>
              <ButtonItem layout="below" disabled={busy} onClick={() => choose(pending.mode, true)}>
                {"Confirm: switch anyway"}
              </ButtonItem>
            </PanelSectionRow>
            <PanelSectionRow>
              <ButtonItem layout="below" disabled={busy} onClick={() => setPending(null)}>
                {"Cancel"}
              </ButtonItem>
            </PanelSectionRow>
          </>
        )}
      </PanelSection>

      <PanelSection title="Status">
        <Row label="Glasses reachable" value={state ? (reachable ? `yes${device?.model ? ` (${device.name ?? device.model})` : ""}` : "no") : "…"} good={state ? reachable : undefined} />
        <Row label="head mouse" value={state ? `${state.units.headlook?.enabled ?? "?"} / ${state.units.headlook?.active ?? "?"}` : "…"} />
        <Row label="SteamVR socket" value={state ? `${state.units.socket?.enabled ?? "?"} / ${state.units.socket?.active ?? "?"}` : "…"} />
        <Row label="Runtime" value={version ? `installed (${version})` : state?.runtime?.installed ? "installed" : "not installed"} good={state ? !!(version || state.runtime.installed) : undefined} />
      </PanelSection>

      {state && !state.runtime.installed && (
        <PanelSection title="Runtime missing">
          <PanelSectionRow>
            <div style={{ fontSize: "0.85em", opacity: 0.85 }}>
              {"The xreal-linux runtime is not installed. It is needed before either mode can run."}
            </div>
          </PanelSectionRow>
          <PanelSectionRow>
            <ButtonItem layout="below" disabled={busy} onClick={install}>
              {`Install runtime ${state.expected_version}`}
            </ButtonItem>
          </PanelSectionRow>
        </PanelSection>
      )}

      {state && state.runtime.installed && !reportUsable && (
        <PanelSection title="Runtime update needed">
          <PanelSectionRow>
            <div style={{ fontSize: "0.85em", opacity: 0.85 }}>
              {"The installed runtime did not report a usable device/capability report, so it is too old for this panel."}
            </div>
          </PanelSectionRow>
          <PanelSectionRow>
            <ButtonItem layout="below" disabled={busy} onClick={install}>
              {`Update runtime ${state.expected_version}`}
            </ButtonItem>
          </PanelSectionRow>
        </PanelSection>
      )}

      <PanelSection title="Actions">
        <PanelSectionRow>
          <ButtonItem layout="below" disabled={busy} onClick={refresh}>
            {"Refresh"}
          </ButtonItem>
        </PanelSectionRow>
      </PanelSection>
    </>
  );
}

export default definePlugin(() => {
  return {
    name: "XREAL",
    titleView: <div className={staticClasses.Title}>XREAL</div>,
    content: <Content />,
    icon: <FaGlasses />,
    onDismount() {},
  };
});
