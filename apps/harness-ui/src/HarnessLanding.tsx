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
        <svg viewBox="0 0 860 240" fill="none" xmlns="http://www.w3.org/2000/svg">
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

          <rect x="10" y="40" width="110" height="50" rx="10" fill="#161b22" stroke="#484f58" strokeWidth="1.5" />
          <text x="65" y="62" textAnchor="middle" fill="#c9d1d9" fontSize="11" fontWeight="700">{t('landing.userRequest')}</text>
          <text x="65" y="77" textAnchor="middle" fill="#484f58" fontSize="10">{t('landing.featureGoal')}</text>
          <line x1="120" y1="65" x2="160" y2="65" stroke="#484f58" strokeWidth="1.5" markerEnd="url(#arrow)" />

          <rect x="165" y="35" width="110" height="60" rx="10" fill="#161b22" stroke="#d29922" strokeWidth="1.5" />
          <text x="220" y="60" textAnchor="middle" fill="#d29922" fontSize="11" fontWeight="700">{t('landing.planLabel')}</text>
          <text x="220" y="77" textAnchor="middle" fill="#8b949e" fontSize="10">{t('landing.planDesc')}</text>

          <line x1="275" y1="65" x2="320" y2="65" stroke="#484f58" strokeWidth="1.5" markerEnd="url(#arrow)" />

          <rect x="325" y="35" width="110" height="60" rx="10" fill="#161b22" stroke="#58a6ff" strokeWidth="1.5" />
          <text x="380" y="60" textAnchor="middle" fill="#58a6ff" fontSize="11" fontWeight="700">{t('landing.buildLabel')}</text>
          <text x="380" y="77" textAnchor="middle" fill="#8b949e" fontSize="10">{t('landing.buildDesc')}</text>

          <line x1="435" y1="65" x2="480" y2="65" stroke="#484f58" strokeWidth="1.5" markerEnd="url(#arrow)" />

          <rect x="485" y="35" width="120" height="60" rx="10" fill="#161b22" stroke="#8b5cf6" strokeWidth="1.5" />
          <text x="545" y="60" textAnchor="middle" fill="#8b5cf6" fontSize="11" fontWeight="700">{t('landing.evaluateLabel')}</text>
          <text x="545" y="77" textAnchor="middle" fill="#8b949e" fontSize="10">{t('landing.evaluateDesc')}</text>

          <line x1="605" y1="65" x2="690" y2="65" stroke="#3fb950" strokeWidth="1.5" markerEnd="url(#arrow-green)" />
          <text x="648" y="57" textAnchor="middle" fill="#3fb950" fontSize="10" fontWeight="600">{t('landing.pass')}</text>

          <rect x="695" y="35" width="140" height="60" rx="10" fill="#161b22" stroke="#3fb950" strokeWidth="1.5" />
          <text x="765" y="60" textAnchor="middle" fill="#3fb950" fontSize="11" fontWeight="700">{t('landing.nextFeature')}</text>
          <text x="765" y="77" textAnchor="middle" fill="#8b949e" fontSize="10">{t('landing.orComplete')}</text>

          <line x1="545" y1="95" x2="545" y2="140" stroke="#f85149" strokeWidth="1.5" markerEnd="url(#arrow-red)" />
          <text x="560" y="125" fill="#f85149" fontSize="10" fontWeight="600">{t('landing.fail')}</text>

          <rect x="485" y="145" width="120" height="60" rx="10" fill="#161b22" stroke="#f85149" strokeWidth="1.5" />
          <text x="545" y="170" textAnchor="middle" fill="#f85149" fontSize="11" fontWeight="700">{t('landing.repairLabel')}</text>
          <text x="545" y="187" textAnchor="middle" fill="#8b949e" fontSize="10">{t('landing.repairDesc')}</text>

          <path d="M485 175 L380 175 L380 100" stroke="#58a6ff" strokeWidth="1.5" strokeDasharray="6 3" markerEnd="url(#arrow-blue)" fill="none" />
          <text x="422" y="168" fill="#58a6ff" fontSize="10" fontWeight="600">{t('landing.retryLabel')}</text>

          <line x1="605" y1="175" x2="690" y2="175" stroke="#f85149" strokeWidth="1.5" strokeDasharray="4 3" markerEnd="url(#arrow-red)" />
          <text x="648" y="167" textAnchor="middle" fill="#484f58" fontSize="9">{t('landing.maxRetries')}</text>

          <rect x="695" y="145" width="140" height="60" rx="10" fill="#161b22" stroke="#484f58" strokeWidth="1.5" />
          <text x="765" y="170" textAnchor="middle" fill="#f85149" fontSize="11" fontWeight="700">{t('landing.failedLabel')}</text>
          <text x="765" y="187" textAnchor="middle" fill="#8b949e" fontSize="10">{t('landing.orContinue')}</text>

          <text x="10" y="232" fill="#484f58" fontSize="9">{t('landing.diagramFooter')}</text>
        </svg>
      </div>
    </section>
  );
}
