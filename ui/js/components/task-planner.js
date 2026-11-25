import { store } from '../store.js';

export class TaskPlanner extends HTMLElement {
    connectedCallback() {
        this.innerHTML = `
            <div class="card">
                <h2><span>⏱</span> <span data-i18n="tasks.title">Planificateur de Tâches Périodiques</span></h2>
                <p class="card-desc" data-i18n="tasks.desc">Ordonnancement autonome des missions récurrentes délégué au planificateur système natif de l'hôte.</p>

                <div style="display: grid; grid-template-columns: 1.2fr 160px 2fr auto; gap: 0.75rem; margin-bottom: 0.75rem;">
                    <input type="text" id="task-name" data-i18n-placeholder="tasks.name_placeholder" placeholder="Nom (ex: sync_veille_techno)" />
                    <input type="text" id="task-cron" placeholder="0 2 * * * (UTC)" value="0 2 * * *" style="font-family: var(--font-mono);" />
                    <input type="text" id="task-prompt" data-i18n-placeholder="tasks.prompt_placeholder" placeholder="Instruction agentique à déclencher..." />
                    <button class="btn-primary" id="btn-add-task">
                        <span>+</span> <span data-i18n="tasks.add_btn">Programmer</span>
                    </button>
                </div>
                <div id="task-feedback" style="min-height: 1.25rem; margin-bottom: 0.75rem; font-size: 0.82rem; font-family: var(--font-mono);"></div>

                <table>
                    <thead>
                        <tr>
                            <th style="width: 190px;" data-i18n="tasks.col_name">Nom de la Tâche</th>
                            <th style="width: 220px;" data-i18n="tasks.col_freq">Fréquence (UTC)</th>
                            <th data-i18n="tasks.col_prompt">Instruction Déclenchée</th>
                            <th style="width: 190px;" data-i18n="tasks.col_last">Exécutions (UTC)</th>
                            <th style="width: 150px;" data-i18n="tasks.col_status">Statut & Action</th>
                        </tr>
                    </thead>
                    <tbody id="task-table-body">
                        <tr><td colspan="5" data-i18n="tasks.loading" style="color: var(--text-muted); text-align: center;">Chargement des tâches...</td></tr>
                    </tbody>
                </table>
            </div>
        `;
        if (window.applyI18n) window.applyI18n(this);

        this.querySelector('#btn-add-task').addEventListener('click', () => this.addTask());
        this.loadTasks();
    }

    async loadTasks() {
        const tbody = this.querySelector('#task-table-body');
        try {
            const tasks = await store.getTasks('default_workspace');
            tbody.innerHTML = '';
            if (!tasks || tasks.length === 0) {
                tbody.innerHTML = '<tr><td colspan="5" style="color: var(--text-muted); text-align: center; padding: 2rem;">Aucune tâche programmée dans l\'OS.</td></tr>';
                return;
            }
            tasks.forEach(t => {
                const tr = document.createElement('tr');
                const descHtml = t.cron_desc ? `<div style="font-size: 0.74rem; color: var(--text-muted); margin-top: 2px;">${t.cron_desc}</div>` : '';
                const nextHtml = t.next_run_utc ? `<div style="font-size: 0.73rem; color: var(--accent-cyan);">Proch.: ${t.next_run_utc}</div>` : '';
                tr.innerHTML = `
                    <td><strong style="color: var(--text-primary);">${t.name}</strong></td>
                    <td><code style="color: var(--accent-cyan); font-family: var(--font-mono);">${t.cron_expr}</code>${descHtml}</td>
                    <td style="color: var(--text-secondary);">${t.prompt}</td>
                    <td style="color: var(--text-muted); font-size: 0.8rem; font-family: var(--font-mono);"><div>Dern.: ${t.last_run_at || 'Jamais'}</div>${nextHtml}</td>
                    <td style="display: flex; gap: 0.4rem; align-items: center;">
                        <button class="btn-bubble-action" data-act="toggle" style="cursor: pointer;">
                            <span class="badge ${t.is_active ? 'badge-emerald' : 'badge-amber'}">${t.is_active ? 'Actif' : 'Pause'}</span>
                        </button>
                        <button class="btn-bubble-action" data-act="delete" title="Supprimer" style="cursor: pointer; color: var(--accent-rose);">✕</button>
                    </td>
                `;
                tr.querySelector('[data-act="toggle"]').addEventListener('click', async () => {
                    await store.toggleTask(t.id, !t.is_active);
                    this.loadTasks();
                });
                tr.querySelector('[data-act="delete"]').addEventListener('click', async () => {
                    await store.deleteTask(t.id);
                    this.loadTasks();
                });
                tbody.appendChild(tr);
            });
        } catch (e) {
            tbody.innerHTML = `<tr><td colspan="5" style="color: var(--accent-rose); text-align: center;">Erreur: ${e.message}</td></tr>`;
        }
    }

    async addTask() {
        const name = this.querySelector('#task-name').value.trim();
        const cron_expr = this.querySelector('#task-cron').value.trim();
        const prompt = this.querySelector('#task-prompt').value.trim();
        const fb = this.querySelector('#task-feedback');
        if (!name || !cron_expr || !prompt) return;

        try {
            const res = await store.saveTask({
                project_id: 'default_workspace',
                name,
                cron_expr,
                prompt
            });
            if (fb) {
                fb.style.color = 'var(--accent-emerald)';
                fb.textContent = `✓ Tâche programmée (${res.cron_desc || cron_expr})`;
            }
            this.querySelector('#task-name').value = '';
            this.querySelector('#task-prompt').value = '';
            this.loadTasks();
        } catch (e) {
            if (fb) {
                fb.style.color = 'var(--accent-rose)';
                fb.textContent = `✗ ${e.message}`;
            }
        }
    }
}
customElements.define('task-planner', TaskPlanner);
