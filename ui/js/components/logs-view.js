export class LogsView extends HTMLElement {
    constructor() {
        super();
        this.lastId = 0;
        this.entries = [];
        this.timer = null;
        this.filter = '';
        this.autoScroll = true;
    }

    connectedCallback() {
        if (this._initialized) return;
        this._initialized = true;

        this.innerHTML = `
            <div class="card" style="height: calc(100vh - 120px); display: flex; flex-direction: column; margin-bottom: 0; padding: 1.25rem;">
                <div style="display: flex; justify-content: space-between; align-items: center; gap: 0.75rem; flex-wrap: wrap; border-bottom: 1px solid var(--border-subtle); padding-bottom: 0.85rem; margin-bottom: 0.85rem;">
                    <div>
                        <h2 style="display: flex; align-items: center; gap: 0.5rem; margin: 0;">
                            <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="var(--accent-cyan)" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="4 17 10 11 4 5"></polyline><line x1="12" y1="19" x2="20" y2="19"></line></svg>
                            <span data-i18n="logs.title">Logs Moteur en Direct</span>
                        </h2>
                        <p class="card-desc" data-i18n="logs.desc" style="margin: 0.2rem 0 0 0;">Flux temps réel de ReActEngine (pensées, appels d'outils, cache, anti-stall).</p>
                    </div>
                    <div style="display: flex; align-items: center; gap: 0.6rem; flex-wrap: wrap;">
                        <input type="text" id="logs-filter" data-i18n-placeholder="logs.filter_placeholder" placeholder="Filtrer (ex: TOOL, WARN, web_search)..." style="height: 32px; padding: 0 0.65rem; font-size: 0.78rem; font-family: var(--font-mono); min-width: 220px;" />
                        <label style="display: inline-flex; align-items: center; gap: 0.35rem; font-size: 0.78rem; color: var(--text-muted); cursor: pointer;">
                            <input type="checkbox" id="logs-autoscroll" checked />
                            <span data-i18n="logs.autoscroll">Auto-scroll</span>
                        </label>
                        <button id="btn-clear-logs" data-i18n="logs.clear" style="background: rgba(244, 63, 94, 0.12); border: 1px solid rgba(244, 63, 94, 0.35); color: #fda4af; padding: 0.35rem 0.75rem; border-radius: var(--radius-sm); cursor: pointer; font-size: 0.78rem;">
                            Vider
                        </button>
                    </div>
                </div>

                <div id="logs-stream" style="flex: 1; overflow-y: auto; background: rgba(2, 6, 23, 0.75); border: 1px solid var(--border-subtle); border-radius: var(--radius-sm); padding: 0.75rem 1rem; font-family: var(--font-mono); font-size: 0.78rem; line-height: 1.55;">
                    <div data-i18n="logs.waiting" style="color: var(--text-muted);">En attente des logs d'exécution...</div>
                </div>
            </div>
        `;
        if (window.applyI18n) window.applyI18n(this);

        this.querySelector('#logs-filter').addEventListener('input', (e) => {
            this.filter = e.target.value.toLowerCase();
            this.renderEntries();
        });
        this.querySelector('#logs-autoscroll').addEventListener('change', (e) => {
            this.autoScroll = e.target.checked;
        });
        this.querySelector('#btn-clear-logs').addEventListener('click', async () => {
            try { await fetch('/api/logs/clear', { method: 'POST' }); } catch (_) {}
            this.entries = [];
            this.renderEntries();
        });

        this.pollLogs();
        this.timer = setInterval(() => this.pollLogs(), 1000);
    }

    disconnectedCallback() {
        if (this.timer) {
            clearInterval(this.timer);
            this.timer = null;
        }
    }

    async pollLogs() {
        try {
            const res = await fetch(`/api/logs?since=${this.lastId}`);
            if (!res.ok) return;
            const data = await res.json();
            if (Array.isArray(data.logs) && data.logs.length > 0) {
                for (const entry of data.logs) {
                    this.entries.push(entry);
                    if (entry.id > this.lastId) this.lastId = entry.id;
                }
                if (this.entries.length > 500) {
                    this.entries = this.entries.slice(-500);
                }
                this.renderEntries();
            }
        } catch (_) {}
    }

    renderEntries() {
        const box = this.querySelector('#logs-stream');
        if (!box) return;
        const colors = {
            INFO: '#38bdf8',
            THOUGHT: '#a5b4fc',
            TOOL: '#fbbf24',
            CACHE: '#34d399',
            OUTPUT: '#94a3b8',
            WARN: '#fb7185',
            ANSWER: '#4ade80'
        };
        const visible = this.filter
            ? this.entries.filter(e =>
                e.level.toLowerCase().includes(this.filter) ||
                e.message.toLowerCase().includes(this.filter))
            : this.entries;

        if (visible.length === 0) {
            box.innerHTML = `<div style="color: var(--text-muted);">Aucun log à afficher.</div>`;
            return;
        }

        const esc = (s) => String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
        box.innerHTML = visible.map(e => {
            const c = colors[e.level] || '#cbd5e1';
            return `<div style="margin-bottom: 0.25rem; word-break: break-word;">
                <span style="color: #64748b;">[${esc(e.timestamp)}]</span>
                <span style="color: ${c}; font-weight: 700; display: inline-block; min-width: 68px;">${esc(e.level)}</span>
                <span style="color: #e2e8f0;">${esc(e.message)}</span>
            </div>`;
        }).join('');

        if (this.autoScroll) {
            box.scrollTop = box.scrollHeight;
        }
    }
}

customElements.define('logs-view', LogsView);
