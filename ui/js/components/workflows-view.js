import { WorkflowStudio, createEmptyWorkflow } from './workflow-studio.js';

export class WorkflowsView extends HTMLElement {
    constructor() {
        super();
        this.workflows = [];
        this.runs = [];
        this.folderFilter = 'ALL';
        this.studio = null;
    }

    connectedCallback() {
        if (this._initialized) return;
        this._initialized = true;

        this.innerHTML = `
            <div style="display: flex; flex-direction: column; gap: 1.25rem;">
                <div class="card" style="margin-bottom: 0;">
                    <div style="display: flex; justify-content: space-between; align-items: center; gap: 0.75rem; flex-wrap: wrap; border-bottom: 1px solid var(--border-subtle); padding-bottom: 0.85rem; margin-bottom: 1rem;">
                        <div>
                            <h2 style="display: flex; align-items: center; gap: 0.5rem; margin: 0;">
                                <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="var(--accent-cyan)" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="22 12 18 12 15 21 9 3 6 12 2 12"></polyline></svg>
                                <span data-i18n="wf.title">Orchestrateur de Workflows WFL</span>
                            </h2>
                            <p class="card-desc" data-i18n="wf.desc" style="margin: 0.25rem 0 0 0;">Gestion des flux déterministes (.wf.yaml) : dossiers, activation, scellage SHA-256, cron et audit SQLite WAL.</p>
                        </div>
                        <div style="display: flex; gap: 0.5rem; align-items: center;">
                            <select id="wf-folder-filter" style="background: rgba(15, 23, 42, 0.9); border: 1px solid var(--border-subtle); color: var(--accent-cyan); border-radius: var(--radius-sm); height: 34px; padding: 0 0.7rem; font-family: var(--font-mono); font-size: 0.8rem;">
                                <option value="ALL">ALL</option>
                            </select>
                            <button class="btn-primary" id="btn-new-wf-studio" style="padding: 0.35rem 0.85rem; font-size: 0.8rem; background: #10b981; border-color: #059669;">+ Nouveau Flux</button>
                            <button class="btn-primary" id="btn-refresh-wf" style="padding: 0.35rem 0.85rem; font-size: 0.8rem;" data-i18n="art.refresh">Rafraîchir</button>
                        </div>
                    </div>

                    <table>
                        <thead>
                            <tr>
                                <th data-i18n="wf.col_name">Workflow</th>
                                <th style="width: 130px;" data-i18n="wf.col_folder">Dossier</th>
                                <th style="width: 100px;" data-i18n="wf.col_seal">Statut</th>
                                <th style="width: 100px;" data-i18n="wf.col_active">Actif</th>
                                <th style="width: 130px;" data-i18n="wf.col_cron">Cron</th>
                                <th style="width: 310px;" data-i18n="wf.col_actions">Actions</th>
                            </tr>
                        </thead>
                        <tbody id="wf-table-body">
                            <tr><td colspan="6" style="color: var(--text-muted); text-align: center; padding: 1.5rem;" data-i18n="wf.loading">Chargement des workflows...</td></tr>
                        </tbody>
                    </table>
                </div>

                <div class="card" id="wf-studio-card" style="margin-bottom: 0;">
                    <div style="display: flex; justify-content: space-between; align-items: center; margin-bottom: 0.65rem;">
                        <h2 style="margin: 0; font-size: 0.95rem; color: var(--accent-cyan);">🧩 Studio Visuel WFL (Fortra Automate Mode)</h2>
                    </div>
                    <div id="wf-studio-mount"></div>
                </div>

                <div style="display: grid; grid-template-columns: 1fr 1fr; gap: 1.25rem;">
                    <div class="card" style="margin-bottom: 0;">
                        <h2 data-i18n="wf.runs_title" style="margin-bottom: 0.65rem;">Historique des Runs WFL</h2>
                        <div id="wf-runs-list" style="max-height: 230px; overflow-y: auto; display: flex; flex-direction: column; gap: 0.45rem;"></div>
                    </div>
                    <div class="card" style="margin-bottom: 0;">
                        <h2 data-i18n="wf.inspector_title" style="margin-bottom: 0.65rem;">Inspecteur StepLogs (SQLite WAL)</h2>
                        <pre id="wf-run-inspector" style="background: rgba(0,0,0,0.55); border: 1px solid var(--border-subtle); border-radius: var(--radius-sm); padding: 0.75rem; font-family: var(--font-mono); font-size: 0.76rem; color: var(--accent-emerald); height: 210px; overflow-y: auto; margin: 0;">-</pre>
                    </div>
                </div>
            </div>
        `;

        if (window.applyI18n) window.applyI18n(this);
        this.studio = new WorkflowStudio(this.querySelector('#wf-studio-mount'), {
            onSaved: () => this.loadData(),
            onStatus: (data, ok) => {
                const inspector = this.querySelector('#wf-run-inspector');
                if (inspector) {
                    inspector.style.color = ok ? 'var(--accent-emerald)' : 'var(--accent-rose)';
                    inspector.textContent = JSON.stringify(data, null, 2);
                }
            }
        });
        this.querySelector('#btn-new-wf-studio').addEventListener('click', () => {
            const n = `flux_${Math.floor(Date.now() / 1000).toString().slice(-4)}`;
            this.studio.loadWorkflow({ spec: createEmptyWorkflow(n), folder: 'General', status: 'draft' });
            this.querySelector('#wf-studio-card').scrollIntoView({ behavior: 'smooth', block: 'start' });
        });
        this.querySelector('#btn-refresh-wf').addEventListener('click', () => this.loadData());
        this.querySelector('#wf-folder-filter').addEventListener('change', (e) => {
            this.folderFilter = e.target.value;
            this.renderTable();
        });
        window.addEventListener('earnest:view_activated', (e) => {
            if (e.detail?.view === 'workflows') this.loadData();
        });
        this.loadData();
    }

    async loadData() {
        try {
            const res = await fetch('/api/workflows');
            const data = await res.json();
            this.workflows = data.workflows || [];
            this.runs = data.runs || [];
            this.updateFolderFilter();
            this.renderTable();
            this.renderRuns();
        } catch (e) {
            this.querySelector('#wf-table-body').innerHTML = `<tr><td colspan="6" style="color: var(--accent-rose);">${e.message}</td></tr>`;
        }
    }

    updateFolderFilter() {
        const sel = this.querySelector('#wf-folder-filter');
        const folders = ['ALL', ...new Set(this.workflows.map(w => w.folder || 'General'))];
        sel.innerHTML = folders.map(f => `<option value="${f}" ${f === this.folderFilter ? 'selected' : ''}>${f}</option>`).join('');
    }

    async callAction(payload) {
        const inspector = this.querySelector('#wf-run-inspector');
        try {
            const res = await fetch('/api/workflows/action', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify(payload)
            });
            const data = await res.json();
            inspector.style.color = res.ok ? 'var(--accent-emerald)' : 'var(--accent-rose)';
            inspector.textContent = JSON.stringify(data, null, 2);
            await this.loadData();
        } catch (e) {
            inspector.style.color = 'var(--accent-rose)';
            inspector.textContent = e.message;
        }
    }

    renderTable() {
        const tbody = this.querySelector('#wf-table-body');
        const list = this.folderFilter === 'ALL'
            ? this.workflows
            : this.workflows.filter(w => (w.folder || 'General') === this.folderFilter);

        if (!list.length) {
            tbody.innerHTML = `<tr><td colspan="6" style="color: var(--text-muted); text-align: center; padding: 1.5rem;">Aucun workflow.</td></tr>`;
            return;
        }
        tbody.innerHTML = '';
        list.forEach(w => {
            const tr = document.createElement('tr');
            const sealBadge = w.status === 'sealed'
                ? '<span class="badge badge-emerald">sealed</span>'
                : '<span class="badge badge-amber">draft</span>';
            const enBadge = w.enabled
                ? '<span class="badge badge-primary">ON</span>'
                : '<span class="badge" style="background: rgba(244,63,94,0.15); color: #fda4af;">OFF</span>';
            tr.innerHTML = `
                <td><strong style="font-family: var(--font-mono);">${w.name}</strong><div style="font-size: 0.7rem; color: var(--text-muted);">${(w.sha256 || '').slice(0, 12)}</div></td>
                <td><button class="btn-bubble-action" data-act="folder" style="cursor: pointer;">${w.folder || 'General'}</button></td>
                <td>${sealBadge}</td>
                <td><button class="btn-bubble-action" data-act="toggle" style="cursor: pointer;">${enBadge}</button></td>
                <td><button class="btn-bubble-action" data-act="cron" style="cursor: pointer; font-family: var(--font-mono); font-size: 0.75rem;">${w.cron_expr || 'Manual'}</button></td>
                <td style="display: flex; gap: 0.35rem; flex-wrap: wrap;">
                    <button class="btn-bubble-action" data-act="edit" style="cursor: pointer; border-color: rgba(56,189,248,0.5); color: #38bdf8;">✏️ Éditer</button>
                    <button class="btn-bubble-action" data-act="dry_run" style="cursor: pointer;">Dry-Run</button>
                    <button class="btn-bubble-action" data-act="seal" style="cursor: pointer;">Seal</button>
                    <button class="btn-primary" data-act="run" style="padding: 0.25rem 0.6rem; font-size: 0.75rem;">Run</button>
                </td>
            `;
            tr.querySelector('[data-act="edit"]').addEventListener('click', () => this.openInStudio(w));
            tr.querySelector('[data-act="dry_run"]').addEventListener('click', () => this.callAction({ action: 'dry_run', name: w.name }));
            tr.querySelector('[data-act="seal"]').addEventListener('click', () => this.callAction({ action: 'seal', name: w.name }));
            tr.querySelector('[data-act="run"]').addEventListener('click', () => this.callAction({ action: 'run', name: w.name }));
            tr.querySelector('[data-act="toggle"]').addEventListener('click', () => this.callAction({ action: 'set_enabled', name: w.name, enabled: !w.enabled }));
            tr.querySelector('[data-act="folder"]').addEventListener('click', () => {
                const folder = prompt(`Dossier pour '${w.name}':`, w.folder || 'General');
                if (folder !== null) this.callAction({ action: 'set_folder', name: w.name, folder });
            });
            tr.querySelector('[data-act="cron"]').addEventListener('click', () => {
                const cron = prompt(`Cron 5 champs pour '${w.name}' (vide = manuel):`, w.cron_expr || '0 2 * * *');
                if (cron !== null) this.callAction({ action: 'set_cron', name: w.name, cron_expr: cron.trim() || null });
            });
            tbody.appendChild(tr);
        });
    }

    async openInStudio(w) {
        const inspector = this.querySelector('#wf-run-inspector');
        try {
            const res = await fetch(`/api/workflows/detail?name=${encodeURIComponent(w.name)}`);
            const data = await res.json();
            if (!res.ok || !data.spec) throw new Error(data.error || 'Workflow spec introuvable');
            this.studio.loadWorkflow({
                spec: data.spec,
                folder: data.workflow?.folder || w.folder || 'General',
                status: data.workflow?.status || w.status || 'draft'
            });
            this.querySelector('#wf-studio-card').scrollIntoView({ behavior: 'smooth', block: 'start' });
        } catch (e) {
            if (inspector) {
                inspector.style.color = 'var(--accent-rose)';
                inspector.textContent = e.message;
            }
        }
    }

    renderRuns() {
        const box = this.querySelector('#wf-runs-list');
        if (!this.runs.length) {
            box.innerHTML = `<div style="color: var(--text-muted); font-size: 0.82rem;">Aucun run enregistré.</div>`;
            return;
        }
        box.innerHTML = '';
        this.runs.forEach(r => {
            const div = document.createElement('div');
            div.className = 'history-card';
            div.style.cursor = 'pointer';
            const color = r.status === 'success' ? 'var(--accent-emerald)' : 'var(--accent-rose)';
            div.innerHTML = `
                <div style="display: flex; justify-content: space-between; font-family: var(--font-mono); font-size: 0.78rem;">
                    <span><strong>${r.workflow_name}</strong> ${r.dry_run ? '(dry_run)' : '(real)'}</span>
                    <span style="color: ${color};">${r.status.toUpperCase()}</span>
                </div>
                <div style="font-size: 0.72rem; color: var(--text-muted); font-family: var(--font-mono);">${r.run_id}</div>
            `;
            div.addEventListener('click', () => this.callAction({ action: 'inspect_run', run_id: r.run_id }));
            box.appendChild(div);
        });
    }
}
customElements.define('workflows-view', WorkflowsView);
