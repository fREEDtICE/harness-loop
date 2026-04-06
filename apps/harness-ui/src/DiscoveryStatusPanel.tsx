import { useTranslation } from "react-i18next";
import { getDiscoveryPhasePresentation } from "./discoveryPhasePresentation";
import type { WorkspaceDiscoveryPayload } from "./types";

function DiscoveryField({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <div className="cfg-field">
      <label className="cfg-label">{label}</label>
      {children}
    </div>
  );
}

function DiscoveryRow({ children }: { children: React.ReactNode }) {
  return <div className="cfg-row">{children}</div>;
}

export default function DiscoveryStatusPanel({
  discovery,
}: {
  discovery: WorkspaceDiscoveryPayload;
}) {
  const { t } = useTranslation();
  const presentation = getDiscoveryPhasePresentation(
    discovery.status.current_phase,
    t,
  );

  return (
    <div className="cfg-group" data-testid="project-settings-discovery">
      <div className="cfg-group-title">{t("settings.discovery")}</div>
      <DiscoveryRow>
        <DiscoveryField label={t("settings.discoveryPhase")}>
          <input
            data-testid="project-settings-discovery-phase"
            value={presentation.phaseLabel}
            readOnly
          />
        </DiscoveryField>
        <DiscoveryField label={t("settings.profilePath")}>
          <input value={discovery.status.profile_path} readOnly />
        </DiscoveryField>
        <DiscoveryField label={t("settings.lastRefresh")}>
          <input value={discovery.status.last_refreshed_at ?? "—"} readOnly />
        </DiscoveryField>
      </DiscoveryRow>
      <DiscoveryRow>
        <DiscoveryField label={t("settings.scanPath")}>
          <input value={discovery.status.scan_path} readOnly />
        </DiscoveryField>
        <DiscoveryField label={t("settings.refreshError")}>
          <input value={discovery.status.last_refresh_error ?? "—"} readOnly />
        </DiscoveryField>
      </DiscoveryRow>
      <DiscoveryRow>
        <DiscoveryField label={t("settings.discoverySummary")}>
          <textarea
            className="settings-textarea"
            value={discovery.profile_summary ?? "—"}
            readOnly
            rows={3}
            spellCheck={false}
          />
        </DiscoveryField>
      </DiscoveryRow>
    </div>
  );
}
