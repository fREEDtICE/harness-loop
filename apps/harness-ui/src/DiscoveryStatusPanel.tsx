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

function DiscoveryMetric({
  label,
  value,
}: {
  label: string;
  value: number;
}) {
  return (
    <div className="discovery-metric">
      <span className="discovery-metric-value">{value}</span>
      <span className="discovery-metric-label">{label}</span>
    </div>
  );
}

function DiscoveryListSection({
  title,
  items = [],
  code = false,
}: {
  title: string;
  items?: string[];
  code?: boolean;
}) {
  if (items.length === 0) {
    return null;
  }

  return (
    <div className="discovery-card">
      <div className="discovery-card-title">{title}</div>
      <div className="discovery-list">
        {items.map((item) => (
          <span
            key={`${title}:${item}`}
            className={code ? "discovery-chip discovery-chip-code" : "discovery-chip"}
          >
            {item}
          </span>
        ))}
      </div>
    </div>
  );
}

function DiscoveryTextSection({
  title,
  body,
}: {
  title: string;
  body: string | null | undefined;
}) {
  if (!body || !body.trim()) {
    return null;
  }

  return (
    <div className="discovery-card discovery-card-wide">
      <div className="discovery-card-title">{title}</div>
      <p className="discovery-card-copy">{body}</p>
    </div>
  );
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
  const { overview } = discovery;

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
        <DiscoveryField label={t("settings.lastScan")}>
          <input value={discovery.status.last_scanned_at} readOnly />
        </DiscoveryField>
        <DiscoveryField label={t("settings.lastRefresh")}>
          <input value={discovery.status.last_refreshed_at ?? "—"} readOnly />
        </DiscoveryField>
        <DiscoveryField label={t("settings.fallbackProfile")}>
          <input
            value={discovery.status.used_fallback_profile ? t("settings.yes") : t("settings.no")}
            readOnly
          />
        </DiscoveryField>
      </DiscoveryRow>
      <DiscoveryRow>
        <DiscoveryField label={t("settings.profilePath")}>
          <input value={discovery.status.profile_path} readOnly />
        </DiscoveryField>
        <DiscoveryField label={t("settings.scanPath")}>
          <input value={discovery.status.scan_path} readOnly />
        </DiscoveryField>
        <DiscoveryField label={t("settings.evidencePath")}>
          <input value={discovery.status.evidence_path} readOnly />
        </DiscoveryField>
        <DiscoveryField label={t("settings.inferencePath")}>
          <input value={discovery.status.inference_path} readOnly />
        </DiscoveryField>
      </DiscoveryRow>
      <DiscoveryRow>
        <DiscoveryField label={t("settings.workspaceFingerprint")}>
          <input value={discovery.status.workspace_fingerprint} readOnly />
        </DiscoveryField>
        <DiscoveryField label={t("settings.profileFingerprint")}>
          <input value={discovery.status.profile_fingerprint ?? "—"} readOnly />
        </DiscoveryField>
      </DiscoveryRow>
      <DiscoveryRow>
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
      <DiscoveryRow>
        <DiscoveryField label={t("settings.inferenceSummary")}>
          <textarea
            className="settings-textarea"
            value={discovery.inference_summary ?? "—"}
            readOnly
            rows={3}
            spellCheck={false}
          />
        </DiscoveryField>
      </DiscoveryRow>

      <div className="discovery-metrics" data-testid="project-settings-discovery-metrics">
        <DiscoveryMetric label={t("settings.sourceFiles")} value={overview.source_file_count} />
        <DiscoveryMetric label={t("settings.repositories")} value={overview.repository_count} />
        <DiscoveryMetric
          label={t("settings.dependencies")}
          value={overview.dependency_relationship_count}
        />
        <DiscoveryMetric label={t("settings.layers")} value={overview.layer_count} />
        <DiscoveryMetric label={t("settings.apiContracts")} value={overview.api_contract_count} />
        <DiscoveryMetric label={t("settings.userJourneys")} value={overview.user_journey_count} />
        <DiscoveryMetric label={t("settings.e2eCases")} value={overview.e2e_test_case_count} />
        <DiscoveryMetric label={t("settings.authSurfaces")} value={overview.auth_surface_count} />
        <DiscoveryMetric
          label={t("settings.conventions")}
          value={overview.coding_convention_count}
        />
        <DiscoveryMetric
          label={t("settings.buildCommands")}
          value={overview.build_command_count}
        />
        <DiscoveryMetric
          label={t("settings.testCommands")}
          value={overview.test_command_count}
        />
        <DiscoveryMetric label={t("settings.devCommands")} value={overview.dev_command_count} />
        <DiscoveryMetric label={t("settings.inferences")} value={overview.inference_count} />
        <DiscoveryMetric
          label={t("settings.avgInferenceConfidence")}
          value={Math.round((overview.average_inference_confidence ?? 0) * 10) / 10}
        />
      </div>

      <div className="discovery-detail-grid" data-testid="project-settings-discovery-details">
        <DiscoveryTextSection
          title={t("settings.layeringSummary")}
          body={overview.layering_summary}
        />
        <DiscoveryListSection title={t("settings.techStack")} items={overview.tech_stack} />
        <DiscoveryListSection
          title={t("settings.keyConcepts")}
          items={overview.key_concepts}
        />
        <DiscoveryListSection
          title={t("settings.repositories")}
          items={overview.repositories}
        />
        <DiscoveryListSection
          title={t("settings.layerRules")}
          items={overview.layering_rules}
        />
        <DiscoveryListSection
          title={t("settings.layerAmbiguities")}
          items={overview.layering_ambiguities}
        />
        <DiscoveryListSection
          title={t("settings.apiContracts")}
          items={overview.api_contracts}
        />
        <DiscoveryListSection
          title={t("settings.userJourneys")}
          items={overview.user_journeys}
        />
        <DiscoveryListSection
          title={t("settings.e2eCases")}
          items={overview.e2e_test_cases}
        />
        <DiscoveryListSection
          title={t("settings.authSurfaces")}
          items={overview.auth_surfaces}
        />
        <DiscoveryListSection
          title={t("settings.conventions")}
          items={overview.coding_conventions}
        />
        <DiscoveryListSection
          title={t("settings.buildCommands")}
          items={overview.build_commands}
          code
        />
        <DiscoveryListSection
          title={t("settings.testCommands")}
          items={overview.test_commands}
          code
        />
        <DiscoveryListSection
          title={t("settings.devCommands")}
          items={overview.dev_commands}
          code
        />
        <DiscoveryListSection
          title={t("settings.strongestInferences")}
          items={overview.strongest_inferences}
        />
        <DiscoveryListSection
          title={t("settings.weakestInferences")}
          items={overview.weakest_inferences}
        />
        <DiscoveryListSection title={t("settings.risks")} items={overview.risks} />
      </div>
    </div>
  );
}
