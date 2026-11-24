/**
 * Initialise les réglages LSP, le catalogue de serveurs et la bannière d'installation automatique.
 */
export function initLspSettings(state) {
    let pendingBannerLang = null;
    const DISMISSED_STORAGE_KEY = 'earnest_lsp_dismissed_langs';

    function getDismissedLangs() {
        try {
            const raw = localStorage.getItem(DISMISSED_STORAGE_KEY);
            const arr = raw ? JSON.parse(raw) : [];
            return new Set(Array.isArray(arr) ? arr : []);
        } catch (_) {
            return new Set();
        }
    }

    function addDismissedLang(lang) {
        if (!lang) return;
        const set = getDismissedLangs();
        set.add(lang);
        try {
            localStorage.setItem(DISMISSED_STORAGE_KEY, JSON.stringify([...set]));
        } catch (_) { }
    }

    async function saveLspToggleSettings() {
        const prefLspEnabled = document.getElementById('pref-lsp-enabled');
        const prefLspAutoPrompt = document.getElementById('pref-lsp-auto-prompt');
        try {
            await fetch('/api/ide/settings', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({
                    lsp_enabled: prefLspEnabled ? String(prefLspEnabled.checked) : 'true',
                    lsp_auto_prompt: prefLspAutoPrompt ? String(prefLspAutoPrompt.checked) : 'true'
                })
            });
        } catch (e) { }
    }

    async function installLspLanguage(lang, btnEl) {
        const prevText = btnEl ? btnEl.textContent : '';
        if (btnEl) {
            btnEl.disabled = true;
            btnEl.textContent = 'Installation...';
        }
        try {
            const res = await fetch('/api/ide/lsp/install', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ language: lang })
            });
            const data = await res.json();
            if (!res.ok || data.status !== 'ok') {
                throw new Error(data.error || 'Installation failed');
            }
            if (!state.lspCheckedLangs) state.lspCheckedLangs = new Set();
            state.lspCheckedLangs.add(lang);
            const banner = document.getElementById('lsp-install-banner');
            if (banner) banner.style.display = 'none';
            await loadLspCatalog();
        } catch (e) {
            alert(`Échec installation LSP (${lang}): ${e.message}`);
            if (btnEl) {
                btnEl.disabled = false;
                btnEl.textContent = prevText || 'Installer';
            }
        }
    }

    async function loadLspCatalog() {
        const listEl = document.getElementById('lsp-catalog-list');
        if (!listEl) return;
        try {
            const res = await fetch('/api/ide/lsp/catalog');
            if (!res.ok) throw new Error('HTTP ' + res.status);
            const data = await res.json();
            const prefLspEnabled = document.getElementById('pref-lsp-enabled');
            const prefLspAutoPrompt = document.getElementById('pref-lsp-auto-prompt');
            if (prefLspEnabled) prefLspEnabled.checked = data.enabled !== false;
            if (prefLspAutoPrompt) prefLspAutoPrompt.checked = data.auto_prompt !== false;

            const servers = Array.isArray(data.servers) ? data.servers : [];
            if (servers.length === 0) {
                listEl.textContent = 'Aucun serveur LSP configuré.';
                return;
            }
            listEl.innerHTML = '';
            for (const s of servers) {
                const row = document.createElement('div');
                row.style.cssText = 'display:flex;align-items:center;justify-content:space-between;gap:6px;padding:4px 6px;border:1px solid var(--border-subtle);border-radius:4px;background:rgba(255,255,255,0.02);';

                const info = document.createElement('div');
                info.style.cssText = 'min-width:0;flex:1;';
                const title = document.createElement('div');
                title.style.cssText = 'color:var(--text-main);font-weight:600;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;';
                title.textContent = `${s.language} · ${s.display_name}`;
                const sub = document.createElement('div');
                sub.style.cssText = 'color:var(--text-secondary);font-size:0.65rem;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;';
                sub.textContent = s.binary;
                info.appendChild(title);
                info.appendChild(sub);
                row.appendChild(info);

                if (s.installed) {
                    const badge = document.createElement('span');
                    badge.style.cssText = 'color:var(--accent-emerald);font-size:0.68rem;flex-shrink:0;';
                    badge.textContent = s.running ? '● Actif' : '✓ Installé';
                    row.appendChild(badge);
                } else {
                    const btn = document.createElement('button');
                    btn.className = 'btn-primary';
                    btn.style.cssText = 'padding:2px 8px;font-size:0.65rem;flex-shrink:0;';
                    btn.textContent = 'Install';
                    btn.title = s.install_cmd || '';
                    btn.addEventListener('click', () => installLspLanguage(s.language, btn));
                    row.appendChild(btn);
                }
                listEl.appendChild(row);
            }
        } catch (e) {
            listEl.textContent = 'Erreur chargement catalogue LSP.';
        }
    }

    async function checkLspForLanguage(lang) {
        if (!lang || lang === 'plaintext') return;
        if (getDismissedLangs().has(lang)) return;
        if (!state.lspCheckedLangs) state.lspCheckedLangs = new Set();
        if (state.lspCheckedLangs.has(lang)) return;
        state.lspCheckedLangs.add(lang);
        try {
            const res = await fetch(`/api/ide/lsp/status?lang=${encodeURIComponent(lang)}`);
            if (!res.ok) return;
            const data = await res.json();
            if (!data.enabled || !data.auto_prompt || !data.supported || data.installed) return;

            pendingBannerLang = lang;
            const banner = document.getElementById('lsp-install-banner');
            const titleEl = document.getElementById('lsp-banner-title');
            const descEl = document.getElementById('lsp-banner-desc');
            const installBtn = document.getElementById('btn-lsp-banner-install');
            if (!banner || !titleEl || !descEl || !installBtn) return;

            titleEl.textContent = `LSP manquant pour ${lang} (${data.display_name || data.binary})`;
            descEl.textContent = `Commande : ${data.install_cmd}`;
            installBtn.disabled = false;
            installBtn.textContent = 'Installer';
            banner.style.display = 'flex';
        } catch (e) { }
    }

    const prefLspEnabledEl = document.getElementById('pref-lsp-enabled');
    const prefLspAutoPromptEl = document.getElementById('pref-lsp-auto-prompt');
    const btnRefreshLspCatalog = document.getElementById('btn-refresh-lsp-catalog');
    const btnLspBannerInstall = document.getElementById('btn-lsp-banner-install');
    const btnLspBannerDismiss = document.getElementById('btn-lsp-banner-dismiss');

    if (prefLspEnabledEl) prefLspEnabledEl.addEventListener('change', saveLspToggleSettings);
    if (prefLspAutoPromptEl) prefLspAutoPromptEl.addEventListener('change', saveLspToggleSettings);
    if (btnRefreshLspCatalog) btnRefreshLspCatalog.addEventListener('click', loadLspCatalog);
    if (btnLspBannerDismiss) {
        btnLspBannerDismiss.addEventListener('click', () => {
            if (pendingBannerLang) addDismissedLang(pendingBannerLang);
            const banner = document.getElementById('lsp-install-banner');
            if (banner) banner.style.display = 'none';
        });
    }
    if (btnLspBannerInstall) {
        btnLspBannerInstall.addEventListener('click', () => {
            if (pendingBannerLang) installLspLanguage(pendingBannerLang, btnLspBannerInstall);
        });
    }

    return { loadLspCatalog, checkLspForLanguage };
}
