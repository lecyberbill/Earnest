/**
 * Barre de suivi des contrats Mode 2/3 au-dessus du chat + modal Diff Shadow Worktree.
 */
export function updateGovernanceContractsBar(data, rootPath) {
    const bar = document.getElementById('mode2-contracts-bar');
    if (!bar) return;

    const mode = data?.mode || 'normal';
    const contracts = data?.contracts?.contracts || [];
    if (mode === 'normal' && contracts.length === 0) {
        bar.style.display = 'none';
        bar.innerHTML = '';
        return;
    }

    bar.style.display = 'flex';
    bar.innerHTML = '';

    const badgesWrap = document.createElement('div');
    badgesWrap.style.cssText = 'display:flex;align-items:center;gap:4px;flex-wrap:wrap;flex:1;min-width:0;';

    const modeChip = document.createElement('span');
    modeChip.style.cssText = 'font-weight:700;font-size:0.68rem;color:var(--accent-cyan);margin-right:4px;';
    modeChip.textContent = mode === 'constrained' ? '🔒 Mode 2' : '📐 Mode 3';
    badgesWrap.appendChild(modeChip);

    if (contracts.length === 0) {
        const empty = document.createElement('span');
        empty.style.cssText = 'color:var(--text-muted);font-size:0.68rem;';
        empty.textContent = 'Aucun contrat scellé';
        badgesWrap.appendChild(empty);
    } else {
        contracts.forEach(c => {
            const chip = document.createElement('span');
            chip.style.cssText = 'background:rgba(56,189,248,0.12);border:1px solid rgba(56,189,248,0.35);color:#e0f2fe;padding:1px 6px;border-radius:4px;font-size:0.66rem;font-family:var(--font-mono);';
            chip.title = `${c.target_file || ''} (${c.probes_count || 1} sonde(s), ${c.criticality || 'high'})`;
            chip.textContent = c.id || 'C?';
            badgesWrap.appendChild(chip);
        });
    }
    bar.appendChild(badgesWrap);

    const diffBtn = document.createElement('button');
    diffBtn.className = 'icon-btn';
    diffBtn.style.cssText = 'font-size:0.68rem;padding:2px 8px;border:1px solid var(--border-subtle);border-radius:4px;color:var(--accent-cyan);flex-shrink:0;';
    diffBtn.textContent = '🔍 Diff Shadow';
    diffBtn.addEventListener('click', () => openShadowDiffModal(rootPath));
    bar.appendChild(diffBtn);
}

async function openShadowDiffModal(rootPath) {
    let modal = document.getElementById('shadow-diff-modal');
    if (!modal) {
        modal = document.createElement('div');
        modal.id = 'shadow-diff-modal';
        modal.style.cssText = 'position:fixed;inset:0;background:rgba(0,0,0,0.78);z-index:10050;display:none;align-items:center;justify-content:center;backdrop-filter:blur(4px);';
        modal.innerHTML = `
            <div style="background:var(--surface-container,#111827);border:1px solid var(--accent-cyan);border-radius:8px;width:780px;max-width:92vw;max-height:82vh;display:flex;flex-direction:column;overflow:hidden;box-shadow:0 20px 50px rgba(0,0,0,0.6);">
                <div style="display:flex;justify-content:space-between;align-items:center;padding:0.65rem 1rem;border-bottom:1px solid var(--border-subtle);font-family:var(--font-mono);font-size:0.8rem;font-weight:700;color:var(--accent-cyan);">
                    <span>🔍 Shadow Worktree Diff (.ide/shadow_worktree)</span>
                    <button id="shadow-diff-close" style="background:none;border:none;color:var(--text-primary);cursor:pointer;font-size:1rem;">✕</button>
                </div>
                <div id="shadow-diff-files" style="padding:0.5rem 1rem;border-bottom:1px solid var(--border-subtle);font-family:var(--font-mono);font-size:0.72rem;color:var(--text-secondary);"></div>
                <pre id="shadow-diff-body" style="margin:0;padding:0.85rem 1rem;overflow:auto;flex:1;font-family:var(--font-mono);font-size:0.74rem;line-height:1.45;color:#e2e8f0;background:#060e20;white-space:pre-wrap;"></pre>
            </div>
        `;
        document.body.appendChild(modal);
        modal.querySelector('#shadow-diff-close').addEventListener('click', () => { modal.style.display = 'none'; });
        modal.addEventListener('click', (e) => { if (e.target === modal) modal.style.display = 'none'; });
    }
    const filesEl = modal.querySelector('#shadow-diff-files');
    const bodyEl = modal.querySelector('#shadow-diff-body');
    filesEl.textContent = 'Chargement...';
    bodyEl.textContent = '';
    modal.style.display = 'flex';
    try {
        const res = await fetch(`/api/ide/governance/shadow_diff?root=${encodeURIComponent(rootPath || '.')}`);
        const data = await res.json();
        if (!data.shadow_exists) {
            filesEl.textContent = 'Aucun .ide/shadow_worktree actif.';
            bodyEl.textContent = '';
            return;
        }
        const files = Array.isArray(data.files) ? data.files : [];
        filesEl.textContent = files.length ? `Fichiers (${files.length}): ${files.join(' · ')}` : 'Aucune modification en attente dans shadow_worktree.';
        bodyEl.textContent = data.diff && data.diff.trim() ? data.diff : '(Pas de diff git — fichiers non suivis ou identiques à HEAD)';
    } catch (e) {
        filesEl.textContent = `Erreur: ${e.message}`;
    }
}
