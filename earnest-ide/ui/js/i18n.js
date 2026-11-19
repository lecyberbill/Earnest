/**
 * Lightweight i18n helper for Earnest IDE (default: 'en', extensible to any locale).
 */
(function () {
    const TRANSLATIONS = {
        en: {
            'app.title': 'Earnest IDE',
            'nav.files': 'Files',
            'nav.search': 'Search',
            'nav.git': 'Source Control',
            'nav.kb': 'Knowledge Base',
        'kb.title': 'EXPERIENCE KNOWLEDGE BASE',
        'kb.truths': 'truths',
            'nav.settings': 'Settings',
            'btn.save': 'Save',
            'btn.run': 'Run',
            'btn.resume': 'Resume',
            'btn.new': 'New',
            'btn.approve_seal': 'Approve & Seal (SHA-256)',
            'btn.reject': 'Reject',
            'btn.cancel': 'Cancel',
            'chat.placeholder': 'Development instruction...',
            'chat.thinking': 'Earnest Copilot is thinking...',
            'chat.in_progress': 'In progress...',
            'chat.see_more': '+ See more',
            'chat.see_less': '- See less',
            'status.model_unknown': 'Model : Unknown',
            'status.model_prefix': 'Model :',
            'history.loading': 'Loading history...',
            'history.empty': 'No previous sessions recorded',
            'gov.modal_title_default': '🛡️ HUMAN ARCHITECTURE VALIDATION',
            'gov.modal_title_arch': '🛡️ ARCHITECTURE & INVARIANTS REVIEW',
            'gov.modal_title_mutation': '⚡ CONTRACT MUTATION REQUEST',
            'gov.badge_mutation': 'Assisted Mutation',
            'gov.btn_approve_short': 'Approve & Seal',
            'gov.btn_validate_mutation': 'Validate Mutation & Recompute Hash',
            'gov.summary_label': 'Proposed executive summary:',
            'gov.spec_complete': 'Full specification',
            'gov.hash_label': 'Computed hash:',
            'gov.seal_notice_prefix': 'By approving, files',
            'gov.seal_notice_mid': 'and',
            'gov.seal_notice_suffix': 'will be sealed read-only (chmod 444) and the project will switch to',
            'gov.seal_notice_mode': 'Mode 2 Constrained',
            'gov.target_contract': 'Target contract:',
            'gov.reason_label': 'Justification:',
            'gov.diff_label': 'Proposed diff on invariants.json:',
            'gov.mode_normal': 'Mode 1: Normal',
            'gov.mode_archi': 'Mode 3: Archi',
            'gov.mode_constrained': 'Mode 2: Constrained 🔒',
            'gov.mode_corrupted': 'Mode 2: Corrupted ⚠️',
            'gov.revalidate_short': 'Revalidate Invariants',
            'gov.revalidate_btn': '⚡ Revalidate Invariants',
            'gov.cockpit_title': '🛡️ Governance & Integrity Invariants Cockpit',
            'gov.cockpit_subtitle': 'Runtime supervision, contract validation and Shadow Sandbox execution',
            'gov.lock_status_label': 'SHA-256 Lock Status',
            'gov.lock_status_locked': 'LOCKED ✓',
            'kb.btn_save_seal': 'Save & Seal',
            'kb.btn_validate_seal': '✓ Validate and Seal this Truth',
            'kb.status_unsealed': '⏳ Discovered by agent (Not yet sealed)',
            'kb.status_validated_by': 'Validated by',
            'kb.btn_revisit': '⚠️ Downgrade / Mark for review',
            'review.ai_applied': '⚡ AI modification applied',
            'review.accept': '✓ Accept',
            'review.reject': '✕ Reject (Ctrl+Z)',
        },
        fr: {
            'app.title': 'Earnest IDE',
            'nav.files': 'Fichiers',
            'nav.search': 'Recherche',
            'nav.git': 'Contrôle de source',
            'nav.kb': 'Base de connaissances',
        'kb.title': 'BASE DE CONNAISSANCES D\'EXPÉRIENCE',
        'kb.truths': 'vérités',
            'nav.settings': 'Paramètres',
            'btn.save': 'Sauvegarder',
            'btn.run': 'Exécuter',
            'btn.resume': 'Reprendre',
            'btn.new': 'Nouveau',
            'btn.approve_seal': 'Approuver & Sceller (SHA-256)',
            'btn.reject': 'Rejeter',
            'btn.cancel': 'Annuler',
            'chat.placeholder': 'Consigne de développement...',
            'chat.thinking': 'Earnest Copilote réfléchit...',
            'chat.in_progress': 'En cours…',
            'chat.see_more': '+ Voir plus',
            'chat.see_less': '- Réduire',
            'status.model_unknown': 'Modèle : Inconnu',
            'status.model_prefix': 'Modèle :',
            'history.loading': 'Chargement historique...',
            'history.empty': 'Aucune session précédente enregistrée',
            'gov.modal_title_default': '🛡️ VALIDATION HUMAINE D\'ARCHITECTURE',
            'gov.modal_title_arch': '🛡️ REVUE D\'ARCHITECTURE & INVARIANTS',
            'gov.modal_title_mutation': '⚡ DEMANDE DE MUTATION DE CONTRAT',
            'gov.badge_mutation': 'Mutation Assistée',
            'gov.btn_approve_short': 'Approuver & Sceller',
            'gov.btn_validate_mutation': 'Valider Mutation & Recalculer Hash',
            'gov.summary_label': 'Résumé exécutif proposé :',
            'gov.spec_complete': 'Spécification complète',
            'gov.hash_label': 'Hash calculé :',
            'gov.seal_notice_prefix': 'En approuvant, les fichiers',
            'gov.seal_notice_mid': 'et',
            'gov.seal_notice_suffix': 'seront scellés en lecture seule (chmod 444) et le projet basculera en',
            'gov.seal_notice_mode': 'Mode 2 Contraint',
            'gov.target_contract': 'Contrat visé :',
            'gov.reason_label': 'Justification :',
            'gov.diff_label': 'Diff proposé sur invariants.json :',
            'gov.mode_normal': 'Mode 1: Normal',
            'gov.mode_archi': 'Mode 3: Archi',
            'gov.mode_constrained': 'Mode 2: Contraint 🔒',
            'gov.mode_corrupted': 'Mode 2: Corrompu ⚠️',
            'gov.revalidate_short': 'Revalider Invariants',
            'gov.revalidate_btn': '⚡ Revalider les Invariants',
            'gov.cockpit_title': '🛡️ Cockpit de Gouvernance & Invariants d\'Intégrité',
            'gov.cockpit_subtitle': 'Supervision du runtime, validation des contrats et exécution Sandbox Shadow',
            'gov.lock_status_label': 'Statut Verrou SHA-256',
            'gov.lock_status_locked': 'VERROUILLÉ ✓',
            'kb.btn_save_seal': 'Enregistrer & Sceller',
            'kb.btn_validate_seal': '✓ Valider et Sceller cette Vérité',
            'kb.status_unsealed': '⏳ Découvert par l\'agent (Non encore scellé)',
            'kb.status_validated_by': 'Validé par',
            'kb.btn_revisit': '⚠️ Déclasser / Marquer à réviser',
            'review.ai_applied': '⚡ Modification IA appliquée',
            'review.accept': '✓ Accepter',
            'review.reject': '✕ Rejeter (Ctrl+Z)',
        }
    };

    let currentLang = localStorage.getItem('earnest_lang') || 'en';
    if (!TRANSLATIONS[currentLang]) currentLang = 'en';

    window.t = function (key) {
        return (TRANSLATIONS[currentLang] && TRANSLATIONS[currentLang][key])
            || TRANSLATIONS.en[key]
            || key;
    };

    window.setLang = function (lang) {
        if (!TRANSLATIONS[lang]) return;
        currentLang = lang;
        localStorage.setItem('earnest_lang', lang);
        fetch('/api/ide/kv_state', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ key: 'earnest_lang', value: lang })
        }).catch(() => { });
        window.applyI18n();
    };

    window.getLang = function () {
        return currentLang;
    };

    window.applyI18n = function () {
        document.querySelectorAll('[data-i18n]').forEach((el) => {
            const key = el.getAttribute('data-i18n');
            if (key) el.textContent = window.t(key);
        });
        document.querySelectorAll('[data-i18n-placeholder]').forEach((el) => {
            const key = el.getAttribute('data-i18n-placeholder');
            if (key) el.setAttribute('placeholder', window.t(key));
        });
    };

    document.addEventListener('DOMContentLoaded', () => {
        window.applyI18n();
        const langSelect = document.getElementById('pref-ui-lang');
        if (langSelect) {
            langSelect.value = currentLang;
            langSelect.addEventListener('change', (e) => {
                window.setLang(e.target.value);
            });
        }
    });
})();
