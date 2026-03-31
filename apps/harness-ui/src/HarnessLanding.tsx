import { useTranslation } from "react-i18next";

export default function HarnessLanding() {
  const { t } = useTranslation();
  return (
    <section className="landing" data-testid="landing">
      <div className="landing-hero">
        <h2>{t('landing.entropyTitle')}</h2>
        <p className="landing-subtitle">
          {t('landing.entropySubtitle')}
        </p>
      </div>

      <div className="landing-columns">
        <div className="landing-card">
          <div className="landing-card-header">
            <div className="landing-card-icon entropy">⚠</div>
            <h3>{t('landing.entropyCardTitle')}</h3>
          </div>
          <ul className="landing-list">
            <li>{t('landing.entropyItem0')}</li>
            <li>{t('landing.entropyItem1')}</li>
            <li>{t('landing.entropyItem2')}</li>
            <li>{t('landing.entropyItem3')}</li>
            <li>{t('landing.entropyItem4')}</li>
          </ul>
        </div>

        <div className="landing-card">
          <div className="landing-card-header">
            <div className="landing-card-icon harness">⟳</div>
            <h3>{t('landing.harnessCardTitle')}</h3>
          </div>
          <p className="landing-card-desc">
            {t('landing.harnessDesc')}
          </p>
        </div>
      </div>

      <div className="landing-diagram" data-testid="landing-loop-diagram">
        <svg viewBox="0 0 780 320" fill="none" xmlns="http://www.w3.org/2000/svg">
          <defs>
            <marker id="arrow" markerWidth="8" markerHeight="6" refX="7" refY="3" orient="auto">
              <path d="M0 0 L8 3 L0 6" fill="#484f58" />
            </marker>
            <marker id="arrow-green" markerWidth="8" markerHeight="6" refX="7" refY="3" orient="auto">
              <path d="M0 0 L8 3 L0 6" fill="#3fb950" />
            </marker>
            <marker id="arrow-red" markerWidth="8" markerHeight="6" refX="7" refY="3" orient="auto">
              <path d="M0 0 L8 3 L0 6" fill="#f85149" />
            </marker>
            <marker id="arrow-blue" markerWidth="8" markerHeight="6" refX="7" refY="3" orient="auto">
              <path d="M0 0 L8 3 L0 6" fill="#58a6ff" />
            </marker>
          </defs>

          <rect x="40" y="24" width="120" height="50" rx="10" fill="#161b22" stroke="#484f58" strokeWidth="1.5" />
          <text x="100" y="46" textAnchor="middle" fill="#c9d1d9" fontSize="11" fontWeight="700">{t('landing.userRequest')}</text>
          <text x="100" y="62" textAnchor="middle" fill="#484f58" fontSize="10">{t('landing.featureGoal')}</text>
          <line x1="100" y1="74" x2="100" y2="115" stroke="#484f58" strokeWidth="1.5" markerEnd="url(#arrow)" />

          <rect x="40" y="120" width="120" height="60" rx="10" fill="#161b22" stroke="#d29922" strokeWidth="1.5" />
          <text x="100" y="145" textAnchor="middle" fill="#d29922" fontSize="11" fontWeight="700">{t('landing.planLabel')}</text>
          <text x="100" y="162" textAnchor="middle" fill="#8b949e" fontSize="10">{t('landing.planDesc')}</text>

          <line x1="160" y1="150" x2="215" y2="150" stroke="#484f58" strokeWidth="1.5" markerEnd="url(#arrow)" />

          <rect x="220" y="120" width="120" height="60" rx="10" fill="#161b22" stroke="#58a6ff" strokeWidth="1.5" />
          <text x="280" y="145" textAnchor="middle" fill="#58a6ff" fontSize="11" fontWeight="700">{t('landing.buildLabel')}</text>
          <text x="280" y="162" textAnchor="middle" fill="#8b949e" fontSize="10">{t('landing.buildDesc')}</text>

          <line x1="340" y1="150" x2="395" y2="150" stroke="#484f58" strokeWidth="1.5" markerEnd="url(#arrow)" />

          <rect x="400" y="120" width="120" height="60" rx="10" fill="#161b22" stroke="#8b5cf6" strokeWidth="1.5" />
          <text x="460" y="145" textAnchor="middle" fill="#8b5cf6" fontSize="11" fontWeight="700">{t('landing.evaluateLabel')}</text>
          <text x="460" y="162" textAnchor="middle" fill="#8b949e" fontSize="10">{t('landing.evaluateDesc')}</text>

          <line x1="520" y1="150" x2="615" y2="150" stroke="#3fb950" strokeWidth="1.5" markerEnd="url(#arrow-green)" />
          <text x="568" y="142" textAnchor="middle" fill="#3fb950" fontSize="10" fontWeight="600">{t('landing.pass')}</text>

          <rect x="620" y="120" width="140" height="60" rx="10" fill="#161b22" stroke="#3fb950" strokeWidth="1.5" />
          <text x="690" y="145" textAnchor="middle" fill="#3fb950" fontSize="11" fontWeight="700">{t('landing.nextFeature')}</text>
          <text x="690" y="162" textAnchor="middle" fill="#8b949e" fontSize="10">{t('landing.orComplete')}</text>

          <line x1="460" y1="180" x2="460" y2="225" stroke="#f85149" strokeWidth="1.5" markerEnd="url(#arrow-red)" />
          <text x="475" y="210" fill="#f85149" fontSize="10" fontWeight="600">{t('landing.fail')}</text>

          <rect x="400" y="230" width="120" height="60" rx="10" fill="#161b22" stroke="#f85149" strokeWidth="1.5" />
          <text x="460" y="255" textAnchor="middle" fill="#f85149" fontSize="11" fontWeight="700">{t('landing.repairLabel')}</text>
          <text x="460" y="272" textAnchor="middle" fill="#8b949e" fontSize="10">{t('landing.repairDesc')}</text>

          <path d="M400 260 L280 260 L280 185" stroke="#58a6ff" strokeWidth="1.5" strokeDasharray="6 3" markerEnd="url(#arrow-blue)" fill="none" />
          <text x="330" y="253" fill="#58a6ff" fontSize="10" fontWeight="600">{t('landing.retryLabel')}</text>

          <line x1="520" y1="260" x2="615" y2="260" stroke="#f85149" strokeWidth="1.5" strokeDasharray="4 3" markerEnd="url(#arrow-red)" />
          <text x="568" y="252" textAnchor="middle" fill="#484f58" fontSize="9">{t('landing.maxRetries')}</text>

          <rect x="620" y="230" width="140" height="60" rx="10" fill="#161b22" stroke="#484f58" strokeWidth="1.5" />
          <text x="690" y="255" textAnchor="middle" fill="#f85149" fontSize="11" fontWeight="700">{t('landing.failedLabel')}</text>
          <text x="690" y="272" textAnchor="middle" fill="#8b949e" fontSize="10">{t('landing.orContinue')}</text>

          <text x="40" y="312" fill="#484f58" fontSize="9">{t('landing.diagramFooter')}</text>
        </svg>
      </div>
    </section>
  );
}
