/**
 * Studio Visuel de Flux WFL (Ergonomie type Fortra Automate — 3 panneaux + YAML sync)
 */

export const WF_ACTION_CATALOG = [
    // --- VFS / Fichiers ---
    { action: 'vfs.list', cat: 'VFS / Fichiers', icon: '📂', color: '#38bdf8', label: 'Lister fichiers', defaults: { params: { conn: 'local', path: '.', glob: '', recursive: false } } },
    { action: 'vfs.read', cat: 'VFS / Fichiers', icon: '📄', color: '#38bdf8', label: 'Lire fichier', defaults: { params: { conn: 'local', path: 'input.txt', encoding: 'utf8' } } },
    { action: 'vfs.write', cat: 'VFS / Fichiers', icon: '✍️', color: '#38bdf8', label: 'Écrire fichier', defaults: { params: { conn: 'local', path: 'output.txt', content: '', create_dirs: true } } },
    { action: 'vfs.transfer', cat: 'VFS / Fichiers', icon: '🔁', color: '#38bdf8', label: 'Transférer fichier', defaults: { params: { src: { conn: 'local', path: 'a.txt' }, dst: { conn: 'local', path: 'b.txt' }, overwrite: true } } },
    { action: 'vfs.stat', cat: 'VFS / Fichiers', icon: '🔍', color: '#38bdf8', label: 'Inspecter fichier', defaults: { params: { conn: 'local', path: 'file.txt', compute_sha256: false } } },
    { action: 'vfs.delete', cat: 'VFS / Fichiers', icon: '🗑️', color: '#fb7185', label: 'Supprimer fichier', defaults: { params: { conn: 'local', path: 'tmp.txt' } } },
    // --- Mail ---
    { action: 'mail.send', cat: 'Messagerie', icon: '✉️', color: '#c084fc', label: 'Envoyer Email', defaults: { params: { conn: 'smtp', from: 'bot@local', to: ['ops@local'], subject: 'Rapport WFL', body_text: 'Exécution terminée.' } } },
    { action: 'mail.fetch', cat: 'Messagerie', icon: '📥', color: '#c084fc', label: 'Lire Emails', defaults: { params: { conn: 'imap', folder: 'INBOX', limit: 10, unread_only: true, mark_read: false } } },
    // --- Contrôle de flux ---
    { action: 'flow.if', cat: 'Contrôle de flux', icon: '🔀', color: '#fbbf24', label: 'Condition (If / Else)', defaults: { condition: '${vars.ok} == true', then: [], else: [] } },
    { action: 'flow.each', cat: 'Contrôle de flux', icon: '🔄', color: '#fbbf24', label: 'Boucle (For Each)', defaults: { items: '${vars.items}', as: 'item', concurrency: 1, steps: [] } },
    { action: 'flow.set', cat: 'Contrôle de flux', icon: '📌', color: '#fbbf24', label: 'Définir variable', defaults: { params: { var: 'result', value: 'ok' } } },
    { action: 'flow.sleep', cat: 'Contrôle de flux', icon: '⏱️', color: '#fbbf24', label: 'Pause (Sleep)', defaults: { params: { duration_ms: 1000 } } },
    { action: 'flow.call', cat: 'Contrôle de flux', icon: '🔗', color: '#fbbf24', label: 'Appeler sous-flux', defaults: { params: { workflow_path: 'workflows/sub.wf.yaml', inputs: {} } } },
    { action: 'flow.fail', cat: 'Contrôle de flux', icon: '🛑', color: '#fb7185', label: 'Échec explicite', defaults: { params: { code: 'ERR_FLOW', message: 'Condition critique non remplie' } } },
    // --- Système ---
    { action: 'sys.exec', cat: 'Système & Réseau', icon: '💻', color: '#34d399', label: 'Commande Shell / Script', defaults: { params: { program: 'powershell', args: ['-NoProfile', '-Command', 'Write-Output "OK"'], allow_non_zero: false } } },
    { action: 'sys.http', cat: 'Système & Réseau', icon: '🌐', color: '#34d399', label: 'Requête HTTP / API', defaults: { params: { method: 'GET', url: 'https://httpbin.org/get', parse_json: true } } },
    { action: 'sys.log', cat: 'Système & Réseau', icon: '📝', color: '#34d399', label: 'Journaliser (Log)', defaults: { params: { level: 'info', message: 'Étape atteinte' } } },
];

export function createEmptyWorkflow(name = 'nouveau_flux') {
    return {
        version: '1.0',
        name,
        description: 'Workflow automatisé Earnest WFL',
        triggers: [{ type: 'manual' }],
        connections: { local: { driver: 'local', root: '.' } },
        inputs: {},
        vars: {},
        steps: [
            { id: 'step_1', name: 'Log de démarrage', action: 'sys.log', params: { level: 'info', message: 'Démarrage du flux' } }
        ]
    };
}

function cloneJson(v) {
    return JSON.parse(JSON.stringify(v));
}

function summarizeStep(step) {
    const p = step.params || {};
    switch (step.action) {
        case 'sys.log': return `[${p.level || 'info'}] ${p.message || ''}`;
        case 'sys.exec': return `${p.program || ''} ${(p.args || []).join(' ')}`;
        case 'sys.http': return `${p.method || 'GET'} ${p.url || ''}`;
        case 'flow.if': return `Si ${step.condition || '...'}`;
        case 'flow.each': return `Pour chaque ${step.as || 'item'} dans ${step.items || '...'}`;
        case 'flow.set': return `${p.var || ''} = ${JSON.stringify(p.value ?? '')}`;
        case 'flow.sleep': return `${p.duration_ms || 0} ms`;
        case 'flow.call': return `${p.workflow_path || ''}`;
        case 'flow.fail': return `${p.code || ''}: ${p.message || ''}`;
        case 'vfs.transfer': return `${p.src?.conn || '?'}:${p.src?.path || ''} → ${p.dst?.conn || '?'}:${p.dst?.path || ''}`;
        default: return p.conn ? `${p.conn}:${p.path || p.folder || ''}` : step.action;
    }
}

export class WorkflowStudio {
    constructor(container, { onSaved, onStatus } = {}) {
        this.container = container;
        this.onSaved = onSaved || (() => {});
        this.onStatus = onStatus || (() => {});
        this.spec = createEmptyWorkflow();
        this.folder = 'General';
        this.recordStatus = 'draft';
        this.selectedPath = [0]; // chemin d'indices vers l'étape sélectionnée
        this.mode = 'visual'; // 'visual' | 'yaml'
        this.paletteQuery = '';
        this.stepStatuses = {}; // step_id -> 'ok' | 'error' | 'skipped'
        this.collapsedCats = new Set();
        this.render();
    }

    loadWorkflow({ spec, folder, status, stepStatuses } = {}) {
        this.spec = spec ? cloneJson(spec) : createEmptyWorkflow();
        if (!Array.isArray(this.spec.steps)) this.spec.steps = [];
        if (!Array.isArray(this.spec.triggers)) this.spec.triggers = [{ type: 'manual' }];
        if (!this.spec.connections) this.spec.connections = { local: { driver: 'local', root: '.' } };
        if (!this.spec.vars) this.spec.vars = {};
        this.folder = folder || 'General';
        this.recordStatus = status || 'draft';
        this.stepStatuses = stepStatuses || {};
        this.selectedPath = this.spec.steps.length > 0 ? [0] : null;
        this.render();
    }

    setStepStatuses(logs = []) {
        const map = {};
        for (const l of logs) {
            if (l.step_id) map[l.step_id] = l.status || 'ok';
        }
        this.stepStatuses = map;
        this.renderCanvasAndInspector();
    }

    getListAndIndex(path) {
        if (!Array.isArray(path) || path.length === 0) return null;
        let list = this.spec.steps;
        for (let i = 0; i < path.length - 1; i += 2) {
            const idx = path[i];
            const branch = path[i + 1];
            const parent = list[idx];
            if (!parent) return null;
            if (!Array.isArray(parent[branch])) parent[branch] = [];
            list = parent[branch];
        }
        return { list, index: path[path.length - 1] };
    }

    getSelectedStep() {
        const ref = this.getListAndIndex(this.selectedPath);
        return ref ? ref.list[ref.index] || null : null;
    }

    nextStepId() {
        const used = new Set();
        const walk = (arr) => {
            for (const s of arr || []) {
                if (s.id) used.add(s.id);
                if (s.then) walk(s.then);
                if (s.else) walk(s.else);
                if (s.steps) walk(s.steps);
            }
        };
        walk(this.spec.steps);
        let n = used.size + 1;
        while (used.has(`step_${n}`)) n++;
        return `step_${n}`;
    }

    addAction(actionName, targetList = this.spec.steps, basePath = []) {
        const tpl = WF_ACTION_CATALOG.find(a => a.action === actionName);
        if (!tpl) return;
        const step = {
            id: this.nextStepId(),
            name: tpl.label,
            action: tpl.action,
            ...cloneJson(tpl.defaults)
        };
        targetList.push(step);
        this.selectedPath = [...basePath, targetList.length - 1];
        this.renderCanvasAndInspector();
    }

    moveStep(path, delta) {
        const ref = this.getListAndIndex(path);
        if (!ref) return;
        const { list, index } = ref;
        const target = index + delta;
        if (target < 0 || target >= list.length) return;
        const [item] = list.splice(index, 1);
        list.splice(target, 0, item);
        this.selectedPath = [...path.slice(0, -1), target];
        this.renderCanvasAndInspector();
    }

    duplicateStep(path) {
        const ref = this.getListAndIndex(path);
        if (!ref) return;
        const { list, index } = ref;
        const copy = cloneJson(list[index]);
        copy.id = this.nextStepId();
        list.splice(index + 1, 0, copy);
        this.selectedPath = [...path.slice(0, -1), index + 1];
        this.renderCanvasAndInspector();
    }

    deleteStep(path) {
        const ref = this.getListAndIndex(path);
        if (!ref) return;
        ref.list.splice(ref.index, 1);
        this.selectedPath = this.spec.steps.length > 0 ? [0] : null;
        this.renderCanvasAndInspector();
    }

    render() {
        this.container.innerHTML = `
            <div style="display: flex; flex-direction: column; gap: 0.75rem;">
                <!-- Toolbar Studio -->
                <div style="display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: 0.5rem; padding: 0.65rem 0.85rem; background: rgba(15, 23, 42, 0.85); border: 1px solid var(--border-subtle); border-radius: var(--radius-sm);">
                    <div style="display: flex; align-items: center; gap: 0.5rem; flex-wrap: wrap;">
                        <span style="font-size: 0.75rem; color: var(--text-muted); font-family: var(--font-mono);">FLUX:</span>
                        <input id="wfs-name" type="text" value="${this.spec.name || ''}" style="background: #060e20; border: 1px solid var(--border-subtle); color: #fff; padding: 4px 8px; border-radius: 4px; font-family: var(--font-mono); font-size: 0.82rem; width: 180px;">
                        <input id="wfs-folder" type="text" value="${this.folder || 'General'}" title="Dossier" style="background: #060e20; border: 1px solid var(--border-subtle); color: var(--accent-cyan); padding: 4px 8px; border-radius: 4px; font-family: var(--font-mono); font-size: 0.76rem; width: 110px;">
                        <span class="badge ${this.recordStatus === 'sealed' ? 'badge-emerald' : 'badge-amber'}">${this.recordStatus}</span>
                    </div>
                    <div style="display: flex; align-items: center; gap: 0.4rem; flex-wrap: wrap;">
                        <div style="display: inline-flex; border: 1px solid var(--border-subtle); border-radius: 4px; overflow: hidden;">
                            <button id="wfs-mode-visual" class="btn-bubble-action" style="border:none; border-radius:0; padding: 4px 10px; background: ${this.mode === 'visual' ? 'rgba(56,189,248,0.2)' : 'transparent'}; color: ${this.mode === 'visual' ? '#38bdf8' : 'var(--text-muted)'};">🎨 Visuel</button>
                            <button id="wfs-mode-yaml" class="btn-bubble-action" style="border:none; border-radius:0; padding: 4px 10px; background: ${this.mode === 'yaml' ? 'rgba(56,189,248,0.2)' : 'transparent'}; color: ${this.mode === 'yaml' ? '#38bdf8' : 'var(--text-muted)'};">📜 YAML</button>
                        </div>
                        <button id="wfs-btn-new" class="btn-bubble-action" style="cursor:pointer;">+ Nouveau</button>
                        <button id="wfs-btn-validate" class="btn-bubble-action" style="cursor:pointer;">✓ Valider</button>
                        <button id="wfs-btn-save" class="btn-primary" style="padding: 5px 12px; font-size: 0.78rem;">💾 Enregistrer</button>
                        <button id="wfs-btn-seal" class="btn-bubble-action" style="cursor:pointer; border-color: rgba(16,185,129,0.5); color: #34d399;">🔒 Sceller</button>
                        <button id="wfs-btn-dryrun" class="btn-bubble-action" style="cursor:pointer;">🧪 Dry-Run</button>
                        <button id="wfs-btn-run" class="btn-primary" style="padding: 5px 12px; font-size: 0.78rem; background: #10b981; border-color: #059669;">▶ Run</button>
                    </div>
                </div>

                <!-- Corps 3 panneaux -->
                <div style="display: grid; grid-template-columns: 240px 1fr 310px; gap: 0.75rem; min-height: 520px; align-items: start;">
                    <!-- Gauche : Palette Actions -->
                    <div id="wfs-palette" style="background: rgba(6, 14, 32, 0.75); border: 1px solid var(--border-subtle); border-radius: var(--radius-sm); padding: 0.65rem; max-height: 640px; overflow-y: auto;"></div>
                    <!-- Centre : Canvas Visuel ou YAML -->
                    <div id="wfs-canvas" style="background: rgba(6, 14, 32, 0.55); border: 1px solid var(--border-subtle); border-radius: var(--radius-sm); padding: 0.85rem; min-height: 520px; max-height: 640px; overflow-y: auto;"></div>
                    <!-- Droite : Inspecteur de propriétés -->
                    <div id="wfs-inspector" style="background: rgba(6, 14, 32, 0.75); border: 1px solid var(--border-subtle); border-radius: var(--radius-sm); padding: 0.75rem; max-height: 640px; overflow-y: auto;"></div>
                </div>
            </div>
        `;

        this.bindToolbar();
        this.renderPalette();
        this.renderCanvasAndInspector();
    }

    bindToolbar() {
        this.container.querySelector('#wfs-name').addEventListener('input', (e) => { this.spec.name = e.target.value.trim(); });
        this.container.querySelector('#wfs-folder').addEventListener('input', (e) => { this.folder = e.target.value.trim() || 'General'; });
        this.container.querySelector('#wfs-mode-visual').addEventListener('click', () => this.switchMode('visual'));
        this.container.querySelector('#wfs-mode-yaml').addEventListener('click', () => this.switchMode('yaml'));
        this.container.querySelector('#wfs-btn-new').addEventListener('click', () => {
            const n = `flux_${Math.floor(Date.now() / 1000).toString().slice(-4)}`;
            this.loadWorkflow({ spec: createEmptyWorkflow(n), folder: this.folder, status: 'draft' });
        });
        this.container.querySelector('#wfs-btn-validate').addEventListener('click', () => this.validateCurrent());
        this.container.querySelector('#wfs-btn-save').addEventListener('click', () => this.saveDraft());
        this.container.querySelector('#wfs-btn-seal').addEventListener('click', () => this.runServerAction('seal'));
        this.container.querySelector('#wfs-btn-dryrun').addEventListener('click', () => this.runServerAction('dry_run'));
        this.container.querySelector('#wfs-btn-run').addEventListener('click', () => this.runServerAction('run'));
    }

    async switchMode(nextMode) {
        if (nextMode === this.mode) return;
        if (nextMode === 'yaml') {
            this.mode = 'yaml';
            this.render();
            const ta = this.container.querySelector('#wfs-yaml-editor');
            if (ta) {
                try {
                    const res = await fetch('/api/workflows/validate', {
                        method: 'POST',
                        headers: { 'Content-Type': 'application/json' },
                        body: JSON.stringify({ spec: this.spec })
                    });
                    const data = await res.json();
                    if (data.yaml) ta.value = data.yaml;
                } catch (e) {}
            }
        } else {
            const ta = this.container.querySelector('#wfs-yaml-editor');
            if (ta && ta.value.trim()) {
                try {
                    const res = await fetch('/api/workflows/validate', {
                        method: 'POST',
                        headers: { 'Content-Type': 'application/json' },
                        body: JSON.stringify({ yaml: ta.value })
                    });
                    const data = await res.json();
                    if (!res.ok) {
                        this.onStatus(data, false);
                        return;
                    }
                    if (data.spec) this.spec = data.spec;
                } catch (e) {
                    this.onStatus({ error: e.message }, false);
                    return;
                }
            }
            this.mode = 'visual';
            this.render();
        }
    }

    async validateCurrent() {
        const ta = this.container.querySelector('#wfs-yaml-editor');
        const payload = (this.mode === 'yaml' && ta) ? { yaml: ta.value } : { spec: this.spec };
        try {
            const res = await fetch('/api/workflows/validate', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify(payload)
            });
            const data = await res.json();
            this.onStatus(data, res.ok && data.report?.valid !== false);
            return data;
        } catch (e) {
            this.onStatus({ error: e.message }, false);
        }
    }

    async saveDraft() {
        const ta = this.container.querySelector('#wfs-yaml-editor');
        const payload = (this.mode === 'yaml' && ta)
            ? { yaml: ta.value, folder: this.folder }
            : { spec: this.spec, folder: this.folder };
        try {
            const res = await fetch('/api/workflows/save', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify(payload)
            });
            const data = await res.json();
            this.onStatus(data, res.ok);
            if (res.ok && data.spec) {
                this.spec = data.spec;
                this.recordStatus = data.workflow?.status || 'draft';
                this.onSaved(data.workflow);
            }
            return res.ok;
        } catch (e) {
            this.onStatus({ error: e.message }, false);
            return false;
        }
    }

    async runServerAction(action) {
        if (action === 'seal' || action === 'dry_run') {
            const ok = await this.saveDraft();
            if (!ok) return;
        }
        try {
            const res = await fetch('/api/workflows/action', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ action, name: this.spec.name })
            });
            const data = await res.json();
            this.onStatus(data, res.ok);
            const logs = data?.data?.report?.step_logs;
            if (Array.isArray(logs)) this.setStepStatuses(logs);
            if (action === 'seal' && res.ok) this.recordStatus = 'sealed';
            this.onSaved(null);
        } catch (e) {
            this.onStatus({ error: e.message }, false);
        }
    }

    renderPalette() {
        const pal = this.container.querySelector('#wfs-palette');
        if (!pal) return;
        const q = this.paletteQuery.toLowerCase();
        const grouped = {};
        for (const item of WF_ACTION_CATALOG) {
            if (q && !item.label.toLowerCase().includes(q) && !item.action.toLowerCase().includes(q)) continue;
            if (!grouped[item.cat]) grouped[item.cat] = [];
            grouped[item.cat].push(item);
        }

        pal.innerHTML = `
            <div style="font-size: 0.72rem; font-weight: 700; color: var(--accent-cyan); margin-bottom: 0.45rem; font-family: var(--font-mono);">PALETTE D'ACTIONS</div>
            <input id="wfs-pal-search" type="text" value="${this.paletteQuery}" placeholder="Filtrer action..." style="width: 100%; box-sizing: border-box; background: #0b1326; border: 1px solid var(--border-subtle); color: #fff; padding: 5px 8px; border-radius: 4px; font-size: 0.75rem; margin-bottom: 0.6rem;">
            <div id="wfs-pal-groups" style="display: flex; flex-direction: column; gap: 0.55rem;"></div>
        `;
        const search = pal.querySelector('#wfs-pal-search');
        search.addEventListener('input', (e) => {
            this.paletteQuery = e.target.value;
            this.renderPalette();
            const next = this.container.querySelector('#wfs-pal-search');
            if (next) { next.focus(); next.setSelectionRange(next.value.length, next.value.length); }
        });

        const groupsEl = pal.querySelector('#wfs-pal-groups');
        for (const [cat, items] of Object.entries(grouped)) {
            const box = document.createElement('div');
            const hdr = document.createElement('div');
            hdr.style.cssText = 'font-size:0.68rem; color:var(--text-muted); text-transform:uppercase; font-weight:700; margin-bottom:4px; cursor:pointer;';
            hdr.textContent = `${this.collapsedCats.has(cat) ? '▶' : '▼'} ${cat}`;
            hdr.addEventListener('click', () => {
                if (this.collapsedCats.has(cat)) this.collapsedCats.delete(cat);
                else this.collapsedCats.add(cat);
                this.renderPalette();
            });
            box.appendChild(hdr);
            if (!this.collapsedCats.has(cat)) {
                const list = document.createElement('div');
                list.style.cssText = 'display:flex; flex-direction:column; gap:4px;';
                for (const it of items) {
                    const btn = document.createElement('button');
                    btn.draggable = true;
                    btn.style.cssText = `display:flex; align-items:center; gap:6px; width:100%; text-align:left; background:rgba(255,255,255,0.03); border:1px solid var(--border-subtle); border-left:3px solid ${it.color}; color:var(--text-primary); padding:5px 7px; border-radius:4px; cursor:pointer; font-size:0.74rem;`;
                    btn.innerHTML = `<span>${it.icon}</span><span style="flex:1; overflow:hidden; text-overflow:ellipsis; white-space:nowrap;">${it.label}</span><span style="font-size:0.64rem; color:var(--text-muted); font-family:var(--font-mono);">+</span>`;
                    btn.addEventListener('click', () => this.addAction(it.action));
                    btn.addEventListener('dragstart', (ev) => ev.dataTransfer.setData('text/wfl-action', it.action));
                    list.appendChild(btn);
                }
                box.appendChild(list);
            }
            groupsEl.appendChild(box);
        }
    }

    renderCanvasAndInspector() {
        this.renderCanvas();
        this.renderInspector();
    }

    renderCanvas() {
        const canvas = this.container.querySelector('#wfs-canvas');
        if (!canvas) return;
        if (this.mode === 'yaml') {
            canvas.innerHTML = `<textarea id="wfs-yaml-editor" spellcheck="false" style="width:100%; height:560px; box-sizing:border-box; background:#040914; color:#a5f3fc; border:1px solid var(--border-subtle); border-radius:4px; padding:0.75rem; font-family:var(--font-mono); font-size:0.78rem; line-height:1.45; resize:vertical;"></textarea>`;
            return;
        }

        canvas.innerHTML = '';

        // 1. En-tête Déclencheurs & Connexions
        const meta = document.createElement('div');
        meta.style.cssText = 'display:flex; flex-wrap:wrap; gap:0.5rem; margin-bottom:0.85rem; padding-bottom:0.65rem; border-bottom:1px dashed var(--border-subtle); font-size:0.74rem;';
        const trigList = (this.spec.triggers || []).map((tr, idx) => {
            const label = tr.type === 'cron' ? `⏰ cron: ${tr.expr}` : tr.type === 'vfs_watch' ? `👁️ watch: ${tr.path}` : tr.type === 'mail_watch' ? `📬 mail: ${tr.folder}` : '👆 manual';
            return `<span class="badge badge-primary" style="display:inline-flex;align-items:center;gap:4px;">${label} <button data-del-trig="${idx}" style="background:none;border:none;color:#fda4af;cursor:pointer;padding:0;font-size:0.7rem;">✕</button></span>`;
        }).join('');
        const connList = Object.entries(this.spec.connections || {}).map(([k, c]) => `<span class="badge" style="background:rgba(52,211,153,0.12);color:#34d399;">🔌 ${k} (${c.driver})</span>`).join('');
        const varsList = Object.entries(this.spec.vars || {}).map(([k, v]) => `<span class="badge" style="background:rgba(251,191,36,0.12);color:#fbbf24;display:inline-flex;align-items:center;gap:4px;">📌 ${k}=${JSON.stringify(v)} <button data-del-var="${k}" style="background:none;border:none;color:#fda4af;cursor:pointer;padding:0;font-size:0.7rem;">✕</button></span>`).join('');
        meta.innerHTML = `
            <div style="display:flex;align-items:center;gap:4px;flex-wrap:wrap;">
                <span style="color:var(--text-muted);font-family:var(--font-mono);">TRIGGERS:</span> ${trigList}
                <button id="wfs-add-trig" class="btn-bubble-action" style="font-size:0.68rem;padding:2px 6px;cursor:pointer;">+ Trigger</button>
            </div>
            <div style="display:flex;align-items:center;gap:4px;flex-wrap:wrap;">
                <span style="color:var(--text-muted);font-family:var(--font-mono);">CONNEXIONS:</span> ${connList}
                <button id="wfs-add-conn" class="btn-bubble-action" style="font-size:0.68rem;padding:2px 6px;cursor:pointer;">+ Connexion</button>
            </div>
            <div style="display:flex;align-items:center;gap:4px;flex-wrap:wrap;">
                <span style="color:var(--text-muted);font-family:var(--font-mono);">VARS:</span> ${varsList}
                <button id="wfs-add-var" class="btn-bubble-action" style="font-size:0.68rem;padding:2px 6px;cursor:pointer;">+ Variable</button>
            </div>
        `;
        meta.querySelectorAll('[data-del-var]').forEach(b => b.addEventListener('click', () => {
            delete this.spec.vars[b.getAttribute('data-del-var')];
            this.renderCanvas();
        }));
        meta.querySelector('#wfs-add-var').addEventListener('click', () => {
            const vname = prompt('Nom de la variable :', 'ok');
            if (!vname || !vname.trim()) return;
            const rawVal = prompt(`Valeur initiale pour '${vname.trim()}' (true, 42, "texte", ["a","b"]) :`, 'true');
            if (rawVal === null) return;
            if (!this.spec.vars) this.spec.vars = {};
            try { this.spec.vars[vname.trim()] = JSON.parse(rawVal); } catch (_) { this.spec.vars[vname.trim()] = rawVal; }
            this.renderCanvas();
        });
        meta.querySelectorAll('[data-del-trig]').forEach(b => b.addEventListener('click', () => {
            this.spec.triggers.splice(parseInt(b.getAttribute('data-del-trig'), 10), 1);
            if (this.spec.triggers.length === 0) this.spec.triggers.push({ type: 'manual' });
            this.renderCanvas();
        }));
        meta.querySelector('#wfs-add-trig').addEventListener('click', () => {
            const expr = prompt('Expression cron (laisser vide pour manual) :', '0 */5 * * *');
            if (expr === null) return;
            this.spec.triggers.push(expr.trim() ? { type: 'cron', expr: expr.trim(), timezone: 'UTC' } : { type: 'manual' });
            this.renderCanvas();
        });
        meta.querySelector('#wfs-add-conn').addEventListener('click', () => {
            const cname = prompt('Nom de connexion (ex: s3_prod, smtp, local) :', 'sftp_in');
            if (!cname || !cname.trim()) return;
            const driver = prompt('Driver (local, sftp, ftp, s3, smtp, imap, http) :', 'local') || 'local';
            if (driver === 'local') this.spec.connections[cname.trim()] = { driver: 'local', root: '.' };
            else if (driver === 'http') this.spec.connections[cname.trim()] = { driver: 'http', base_url: 'https://api.example.com' };
            else this.spec.connections[cname.trim()] = { driver, host: 'localhost', user: 'user', password: '' };
            this.renderCanvasAndInspector();
        });
        canvas.appendChild(meta);

        // 2. Liste des étapes
        const stepsWrap = this.renderStepList(this.spec.steps, []);
        canvas.appendChild(stepsWrap);
    }

    renderStepList(list, basePath) {
        const wrap = document.createElement('div');
        wrap.style.cssText = 'display:flex; flex-direction:column; gap:0.45rem;';
        wrap.addEventListener('dragover', (e) => { e.preventDefault(); wrap.style.outline = '1px dashed var(--accent-cyan)'; });
        wrap.addEventListener('dragleave', () => { wrap.style.outline = 'none'; });
        wrap.addEventListener('drop', (e) => {
            e.preventDefault();
            e.stopPropagation();
            wrap.style.outline = 'none';
            const act = e.dataTransfer.getData('text/wfl-action');
            if (act) this.addAction(act, list, basePath);
        });

        if (!list || list.length === 0) {
            const empty = document.createElement('div');
            empty.style.cssText = 'padding:1rem; text-align:center; border:1px dashed var(--border-subtle); border-radius:4px; color:var(--text-muted); font-size:0.75rem;';
            empty.textContent = 'Glissez une action ici ou cliquez dans la palette à gauche.';
            wrap.appendChild(empty);
            return wrap;
        }

        list.forEach((step, idx) => {
            const path = [...basePath, idx];
            const isSel = JSON.stringify(path) === JSON.stringify(this.selectedPath);
            const meta = WF_ACTION_CATALOG.find(a => a.action === step.action) || { icon: '⚙️', color: '#94a3b8', label: step.action };
            const st = this.stepStatuses[step.id];
            const stBadge = st === 'ok' || st === 'success' ? '✅' : st === 'error' || st === 'failed' ? '❌' : st ? '⏭️' : '';

            const card = document.createElement('div');
            card.style.cssText = `background:${isSel ? 'rgba(56,189,248,0.1)' : 'rgba(15,23,42,0.75)'}; border:1px solid ${isSel ? 'var(--accent-cyan)' : 'var(--border-subtle)'}; border-left:4px solid ${meta.color}; border-radius:6px; padding:0.55rem 0.7rem; cursor:pointer;`;
            card.addEventListener('click', (e) => {
                e.stopPropagation();
                this.selectedPath = path;
                this.renderCanvasAndInspector();
            });

            const top = document.createElement('div');
            top.style.cssText = 'display:flex; align-items:center; justify-content:space-between; gap:0.5rem;';
            top.innerHTML = `
                <div style="display:flex; align-items:center; gap:0.45rem; min-width:0;">
                    <span style="font-size:0.7rem; color:var(--text-muted); font-family:var(--font-mono);">#${idx + 1}</span>
                    <span>${meta.icon}</span>
                    <strong style="font-size:0.8rem; color:var(--text-primary);">${step.name || meta.label}</strong>
                    <span style="font-size:0.68rem; font-family:var(--font-mono); color:${meta.color}; background:rgba(255,255,255,0.05); padding:1px 6px; border-radius:4px;">${step.action}</span>
                    ${stBadge ? `<span>${stBadge}</span>` : ''}
                </div>
                <div style="display:flex; gap:3px; flex-shrink:0;">
                    <button data-act="up" title="Monter" class="btn-bubble-action" style="padding:1px 6px;font-size:0.7rem;cursor:pointer;">↑</button>
                    <button data-act="down" title="Descendre" class="btn-bubble-action" style="padding:1px 6px;font-size:0.7rem;cursor:pointer;">↓</button>
                    <button data-act="dup" title="Dupliquer" class="btn-bubble-action" style="padding:1px 6px;font-size:0.7rem;cursor:pointer;">⧉</button>
                    <button data-act="del" title="Supprimer" class="btn-bubble-action" style="padding:1px 6px;font-size:0.7rem;color:#fda4af;cursor:pointer;">✕</button>
                </div>
            `;
            top.querySelector('[data-act="up"]').addEventListener('click', (e) => { e.stopPropagation(); this.moveStep(path, -1); });
            top.querySelector('[data-act="down"]').addEventListener('click', (e) => { e.stopPropagation(); this.moveStep(path, 1); });
            top.querySelector('[data-act="dup"]').addEventListener('click', (e) => { e.stopPropagation(); this.duplicateStep(path); });
            top.querySelector('[data-act="del"]').addEventListener('click', (e) => { e.stopPropagation(); this.deleteStep(path); });
            card.appendChild(top);

            const sub = document.createElement('div');
            sub.style.cssText = 'font-size:0.72rem; color:var(--text-secondary); font-family:var(--font-mono); margin-top:3px; white-space:nowrap; overflow:hidden; text-overflow:ellipsis;';
            sub.textContent = `${step.id} · ${summarizeStep(step)}`;
            card.appendChild(sub);

            if (step.action === 'flow.if') {
                const branches = document.createElement('div');
                branches.style.cssText = 'margin-top:0.5rem; padding-left:0.65rem; border-left:2px solid rgba(251,191,36,0.4); display:flex; flex-direction:column; gap:0.45rem;';
                const thenLbl = document.createElement('div');
                thenLbl.style.cssText = 'font-size:0.68rem; color:#34d399; font-weight:700;';
                thenLbl.textContent = 'THEN (Si vrai)';
                branches.appendChild(thenLbl);
                branches.appendChild(this.renderStepList(step.then || (step.then = []), [...path, 'then']));
                const elseLbl = document.createElement('div');
                elseLbl.style.cssText = 'font-size:0.68rem; color:#fda4af; font-weight:700;';
                elseLbl.textContent = 'ELSE (Sinon)';
                branches.appendChild(elseLbl);
                branches.appendChild(this.renderStepList(step.else || (step.else = []), [...path, 'else']));
                card.appendChild(branches);
            } else if (step.action === 'flow.each') {
                const body = document.createElement('div');
                body.style.cssText = 'margin-top:0.5rem; padding-left:0.65rem; border-left:2px solid rgba(251,191,36,0.4);';
                const lbl = document.createElement('div');
                lbl.style.cssText = 'font-size:0.68rem; color:#fbbf24; font-weight:700; margin-bottom:4px;';
                lbl.textContent = `CORPS DE BOUCLE (${step.as || 'item'})`;
                body.appendChild(lbl);
                body.appendChild(this.renderStepList(step.steps || (step.steps = []), [...path, 'steps']));
                card.appendChild(body);
            }

            wrap.appendChild(card);
            if (idx < list.length - 1) {
                const arrow = document.createElement('div');
                arrow.style.cssText = 'text-align:center; color:var(--text-muted); font-size:0.7rem; line-height:1;';
                arrow.textContent = '↓';
                wrap.appendChild(arrow);
            }
        });
        return wrap;
    }

    renderInspector() {
        const insp = this.container.querySelector('#wfs-inspector');
        if (!insp) return;
        const step = this.getSelectedStep();
        if (!step) {
            insp.innerHTML = `<div style="color:var(--text-muted); font-size:0.78rem; padding:1.5rem; text-align:center;">Sélectionnez une étape sur le canvas pour configurer ses paramètres.</div>`;
            return;
        }

        const connNames = Object.keys(this.spec.connections || {});
        const paramsJson = JSON.stringify(step.params || {}, null, 2);
        insp.innerHTML = `
            <div style="font-size:0.72rem; font-weight:700; color:var(--accent-cyan); margin-bottom:0.6rem; font-family:var(--font-mono);">PROPRIÉTÉS : ${step.action}</div>
            <div style="display:flex; flex-direction:column; gap:0.55rem; font-size:0.75rem;">
                <div>
                    <label style="display:block;color:var(--text-muted);margin-bottom:2px;">ID de l'étape</label>
                    <input id="wfi-id" type="text" value="${step.id || ''}" style="width:100%;box-sizing:border-box;background:#0b1326;border:1px solid var(--border-subtle);color:#fff;padding:5px;border-radius:4px;font-family:var(--font-mono);">
                </div>
                <div>
                    <label style="display:block;color:var(--text-muted);margin-bottom:2px;">Libellé</label>
                    <input id="wfi-name" type="text" value="${step.name || ''}" style="width:100%;box-sizing:border-box;background:#0b1326;border:1px solid var(--border-subtle);color:#fff;padding:5px;border-radius:4px;">
                </div>
                <div>
                    <label style="display:block;color:var(--text-muted);margin-bottom:2px;">Variable de sortie (output)</label>
                    <input id="wfi-output" type="text" value="${step.output || ''}" placeholder="ex: res_http" style="width:100%;box-sizing:border-box;background:#0b1326;border:1px solid var(--border-subtle);color:#fff;padding:5px;border-radius:4px;font-family:var(--font-mono);">
                </div>
                <div id="wfi-specific"></div>
                <div>
                    <label style="display:block;color:var(--text-muted);margin-bottom:2px;">Paramètres JSON (avancé)</label>
                    <textarea id="wfi-params-json" rows="7" spellcheck="false" style="width:100%;box-sizing:border-box;background:#040914;border:1px solid var(--border-subtle);color:#a5f3fc;padding:6px;border-radius:4px;font-family:var(--font-mono);font-size:0.72rem;"></textarea>
                </div>
            </div>
        `;

        insp.querySelector('#wfi-id').addEventListener('input', (e) => { step.id = e.target.value.trim(); this.renderCanvas(); });
        insp.querySelector('#wfi-name').addEventListener('input', (e) => { step.name = e.target.value; this.renderCanvas(); });
        insp.querySelector('#wfi-output').addEventListener('input', (e) => { step.output = e.target.value.trim() || undefined; });

        const specBox = insp.querySelector('#wfi-specific');
        if (step.action === 'flow.if') {
            specBox.innerHTML = `<label style="display:block;color:#fbbf24;margin-bottom:2px;">Condition</label><input id="wfi-cond" type="text" value="${step.condition || ''}" style="width:100%;box-sizing:border-box;background:#0b1326;border:1px solid var(--border-subtle);color:#fff;padding:5px;border-radius:4px;font-family:var(--font-mono);">`;
            specBox.querySelector('#wfi-cond').addEventListener('input', (e) => { step.condition = e.target.value; this.renderCanvas(); });
        } else if (step.action === 'flow.each') {
            specBox.innerHTML = `
                <label style="display:block;color:#fbbf24;margin-bottom:2px;">Liste (items)</label>
                <input id="wfi-items" type="text" value="${step.items || ''}" style="width:100%;box-sizing:border-box;background:#0b1326;border:1px solid var(--border-subtle);color:#fff;padding:5px;border-radius:4px;font-family:var(--font-mono);margin-bottom:4px;">
                <label style="display:block;color:#fbbf24;margin-bottom:2px;">Variable d'élément (as)</label>
                <input id="wfi-as" type="text" value="${step.as || 'item'}" style="width:100%;box-sizing:border-box;background:#0b1326;border:1px solid var(--border-subtle);color:#fff;padding:5px;border-radius:4px;font-family:var(--font-mono);">
            `;
            specBox.querySelector('#wfi-items').addEventListener('input', (e) => { step.items = e.target.value; this.renderCanvas(); });
            specBox.querySelector('#wfi-as').addEventListener('input', (e) => { step.as = e.target.value; this.renderCanvas(); });
        } else if (step.params && typeof step.params === 'object') {
            const quickFields = Object.entries(step.params).filter(([, v]) => typeof v !== 'object' || Array.isArray(v));
            quickFields.forEach(([k, v]) => {
                const row = document.createElement('div');
                row.style.marginBottom = '4px';
                const lbl = document.createElement('label');
                lbl.style.cssText = 'display:block;color:var(--text-muted);margin-bottom:2px;';
                lbl.textContent = Array.isArray(v) ? `${k} (1 par ligne)` : k;
                row.appendChild(lbl);
                if (Array.isArray(v)) {
                    const ta = document.createElement('textarea');
                    ta.rows = Math.min(5, Math.max(2, v.length));
                    ta.value = v.join('\n');
                    ta.style.cssText = 'width:100%;box-sizing:border-box;background:#0b1326;border:1px solid var(--border-subtle);color:#fff;padding:5px;border-radius:4px;font-family:var(--font-mono);font-size:0.72rem;';
                    ta.addEventListener('input', () => {
                        step.params[k] = ta.value.split(/\r?\n/).filter(line => line.length > 0);
                        syncJson();
                        this.renderCanvas();
                    });
                    row.appendChild(ta);
                } else if (k === 'conn' && connNames.length > 0) {
                    const sel = document.createElement('select');
                    sel.style.cssText = 'width:100%;background:#0b1326;border:1px solid var(--border-subtle);color:#fff;padding:5px;border-radius:4px;font-family:var(--font-mono);';
                    [...new Set([v, ...connNames])].filter(Boolean).forEach(cn => {
                        const o = document.createElement('option');
                        o.value = cn; o.textContent = cn;
                        if (cn === v) o.selected = true;
                        sel.appendChild(o);
                    });
                    sel.addEventListener('change', () => { step.params[k] = sel.value; syncJson(); this.renderCanvas(); });
                    row.appendChild(sel);
                } else if (typeof v === 'boolean') {
                    const cb = document.createElement('input');
                    cb.type = 'checkbox'; cb.checked = v;
                    cb.addEventListener('change', () => { step.params[k] = cb.checked; syncJson(); this.renderCanvas(); });
                    row.appendChild(cb);
                } else {
                    const inp = document.createElement('input');
                    inp.type = typeof v === 'number' ? 'number' : 'text';
                    inp.value = (step.action === 'flow.set' && k === 'value' && typeof v !== 'string') ? JSON.stringify(v) : (v ?? '');
                    inp.style.cssText = 'width:100%;box-sizing:border-box;background:#0b1326;border:1px solid var(--border-subtle);color:#fff;padding:5px;border-radius:4px;font-family:var(--font-mono);';
                    inp.addEventListener('input', () => {
                        if (step.action === 'flow.set' && k === 'value') {
                            const raw = inp.value.trim();
                            try { step.params[k] = JSON.parse(raw); } catch (_) { step.params[k] = inp.value; }
                        } else {
                            step.params[k] = typeof v === 'number' ? Number(inp.value) : inp.value;
                        }
                        syncJson();
                        this.renderCanvas();
                    });
                    row.appendChild(inp);
                }
                specBox.appendChild(row);
            });
        }

        const jsonTa = insp.querySelector('#wfi-params-json');
        const syncJson = () => { if (jsonTa) jsonTa.value = JSON.stringify(step.params || {}, null, 2); };
        syncJson();
        jsonTa.addEventListener('change', () => {
            try {
                step.params = JSON.parse(jsonTa.value);
                jsonTa.style.borderColor = 'var(--border-subtle)';
                this.renderCanvas();
            } catch (e) {
                jsonTa.style.borderColor = 'var(--accent-rose)';
            }
        });
    }
}
