export class KbView {
    constructor(containerId) {
        this.container = document.getElementById(containerId);
        this.entries = [];
        this.products = [];
        this.currentFilterKind = '';
        this.currentFilterStatus = '';
        this.currentFilterProduct = '';
        this.searchQuery = '';
        this.selectedEntryId = null;
        this.init();
    }

    init() {
        this.renderLayout();
        this.bindEvents();
        this.loadProducts();
        this.loadEntries();
        window.addEventListener('earnest:lang_changed', () => {
            this.renderLayout();
            this.bindEvents();
            this.filterAndRender();
            const badge = this.container.querySelector('#kb-count-badge');
            const t = (k) => (window.t ? window.t(k) : k);
            if (badge) badge.textContent = `${this.entries.length} ${t('kb.truths')}`;
        });
    }

    renderLayout() {
        const t = (k) => (window.t ? window.t(k) : k);
        this.container.innerHTML = `
            <div style="display: flex; flex-direction: column; height: 100%; width: 100%; background: var(--surface-dim); color: var(--text-primary); font-family: var(--font-sans); overflow: hidden;">
                <!-- Barre d'outils supérieure de la KB (Plein écran) -->
                <div style="display: flex; justify-content: space-between; align-items: center; padding: 0.75rem 1.25rem; background: rgba(19, 27, 46, 0.85); backdrop-filter: blur(12px); border-bottom: 1px solid var(--border-subtle); flex-shrink: 0;">
                    <div style="display: flex; align-items: center; gap: 0.85rem;">
                        <span style="font-size: 1.4rem;">🧠</span>
                        <div>
                            <div style="display: flex; align-items: center; gap: 0.5rem;">
                                <span style="font-weight: 800; font-family: var(--font-mono); font-size: 0.95rem; color: var(--text-primary);">${t('kb.title')}</span>
                                <span id="kb-count-badge" style="background: rgba(128, 131, 255, 0.22); color: var(--accent-cyan); font-family: var(--font-mono); font-size: 0.72rem; padding: 2px 8px; border-radius: 12px; border: 1px solid var(--border-accent); font-weight: 700;">0 ${t('kb.truths')}</span>
                            </div>
                            <div style="font-size: 0.72rem; color: var(--text-muted);">
                                ${t('kb.subtitle')}
                            </div>
                        </div>
                    </div>

                    <div style="display: flex; align-items: center; gap: 0.75rem;">
                        <button id="kb-btn-export" class="icon-btn" style="border: 1px solid var(--border-subtle); padding: 5px 10px; font-size: 0.75rem; border-radius: 4px; gap: 4px;" title="${t('kb.btn_export_title')}">
                            <span>💾</span> ${t('kb.btn_export')}
                        </button>
                        <button id="kb-btn-refresh" class="icon-btn" style="border: 1px solid var(--border-subtle); padding: 5px 10px; font-size: 0.75rem; border-radius: 4px; gap: 4px;" title="${t('kb.btn_refresh_title')}">
                            <span>🔄</span> ${t('kb.btn_refresh')}
                        </button>
                        <button id="kb-btn-new-product" class="icon-btn" style="border: 1px solid var(--accent-cyan); color: var(--accent-cyan); padding: 5px 10px; font-size: 0.75rem; border-radius: 4px; gap: 4px;" title="${t('kb.btn_new_product_title')}">
                            <span>📦</span> ${t('kb.btn_new_product')}
                        </button>
                        <button id="kb-btn-new-rule" class="btn-primary" style="padding: 6px 14px; font-size: 0.78rem; font-weight: 700; display: flex; align-items: center; gap: 0.4rem; border-radius: 4px;">
                            <span>➕</span> ${t('kb.btn_new_rule')}
                        </button>
                    </div>
                </div>

                <!-- Zone de filtrage et recherche -->
                <div style="display: flex; align-items: center; justify-content: space-between; padding: 0.65rem 1.25rem; background: var(--surface-low); border-bottom: 1px solid var(--border-subtle); gap: 1rem; flex-shrink: 0;">
                    <!-- Recherche instantanée -->
                    <div style="position: relative; flex: 1; max-width: 480px;">
                        <input type="text" id="kb-search-input" value="${this.searchQuery || ''}" placeholder="${t('kb.search_placeholder')}" 
                               style="width: 100%; box-sizing: border-box; background: var(--surface-lowest); border: 1px solid var(--border-subtle); color: var(--text-primary); padding: 6px 10px 6px 30px; border-radius: 6px; font-family: var(--font-mono); font-size: 0.8rem;" />
                        <span style="position: absolute; left: 10px; top: 7px; color: var(--text-muted); font-size: 0.8rem;">🔍</span>
                    </div>

                    <!-- Pilules de filtrage -->
                    <div style="display: flex; align-items: center; gap: 0.4rem; font-size: 0.72rem; font-family: var(--font-mono);">
                        <button class="kb-pill ${this.currentFilterKind === '' ? 'active' : ''}" data-kind="">${t('kb.filter_all_types')}</button>
                        <button class="kb-pill ${this.currentFilterKind === 'piege' ? 'active' : ''}" data-kind="piege">${t('kb.filter_pitfalls')}</button>
                        <button class="kb-pill ${this.currentFilterKind === 'regle' ? 'active' : ''}" data-kind="regle">${t('kb.filter_rules')}</button>
                        <button class="kb-pill ${this.currentFilterKind === 'catalogue' ? 'active' : ''}" data-kind="catalogue">${t('kb.filter_catalogs')}</button>
                        <span style="color: var(--border-subtle); margin: 0 4px;">|</span>
                        <button class="kb-pill-status ${this.currentFilterStatus === '' ? 'active' : ''}" data-status="">${t('kb.filter_all_statuses')}</button>
                        <button class="kb-pill-status ${this.currentFilterStatus === 'valide' ? 'active' : ''}" data-status="valide">${t('kb.filter_validated')}</button>
                        <button class="kb-pill-status ${this.currentFilterStatus === 'non_teste' ? 'active' : ''}" data-status="non_teste">${t('kb.filter_discovered')}</button>
                        <select id="kb-filter-product" style="background: var(--surface-lowest); border: 1px solid var(--border-subtle); color: var(--text-primary); padding: 4px 8px; border-radius: 12px; font-size: 0.72rem; font-family: var(--font-mono); margin-left: 4px;">
                            <option value="">${t('kb.filter_all_products')}</option>
                        </select>
                    </div>
                </div>

                <!-- Contenu Principal en 2 Colonnes (Liste de gauche + Vue détaillée / Édition à droite) -->
                <div style="display: grid; grid-template-columns: 420px 1fr; flex: 1; overflow: hidden;">
                    <!-- Colonne Gauche : Liste des Cartes -->
                    <div id="kb-cards-stream" style="overflow-y: auto; padding: 0.85rem; display: flex; flex-direction: column; gap: 0.65rem; border-right: 1px solid var(--border-subtle); background: var(--surface-lowest);">
                        <div style="text-align: center; color: var(--text-muted); font-size: 0.8rem; padding: 3rem;">${t('kb.loading')}</div>
                    </div>

                    <!-- Colonne Droite : Vue Détaillée / Inspecteur & Modèle Avant/Après -->
                    <div id="kb-detail-pane" style="overflow-y: auto; padding: 1.25rem; display: flex; flex-direction: column; gap: 1rem; background: var(--surface-dim);">
                        <div style="text-align: center; color: var(--text-muted); font-size: 0.85rem; padding: 4rem;">
                            ${t('kb.select_hint')}
                        </div>
                    </div>
                </div>

                <!-- Modal Nouvelle Règle -->
                <div id="kb-modal-new" style="display: none; position: fixed; inset: 0; background: rgba(0,0,0,0.8); z-index: 9999; align-items: center; justify-content: center; backdrop-filter: blur(4px);">
                    <div style="background: var(--surface-container); border: 1px solid var(--accent-cyan); width: 560px; max-width: 90vw; border-radius: 8px; padding: 1.5rem; display: flex; flex-direction: column; gap: 1rem; font-family: var(--font-mono); box-shadow: var(--shadow-soft), var(--shadow-glow-cyan);">
                        <div style="display: flex; justify-content: space-between; align-items: center; border-bottom: 1px solid var(--border-subtle); padding-bottom: 0.6rem;">
                            <span style="font-weight: 800; color: var(--accent-cyan); font-size: 0.95rem;">${t('kb.modal_title')}</span>
                            <button id="kb-modal-close" style="background: transparent; border: none; color: var(--text-primary); cursor: pointer; font-size: 1.2rem;">✕</button>
                        </div>

                        <div style="display: grid; grid-template-columns: 1fr 1fr; gap: 0.75rem;">
                            <div>
                                <label style="display: block; font-size: 0.72rem; color: var(--text-muted); margin-bottom: 3px;">${t('kb.field_type')}</label>
                                <select id="kb-new-kind" style="width: 100%; background: var(--surface-lowest); border: 1px solid var(--border-subtle); color: var(--text-primary); padding: 6px; border-radius: 4px; font-size: 0.78rem;">
                                    <option value="regle">${t('kb.opt_rule')}</option>
                                    <option value="piege">${t('kb.opt_pitfall')}</option>
                                    <option value="catalogue">${t('kb.opt_catalog')}</option>
                                </select>
                            </div>
                            <div>
                                <label style="display: block; font-size: 0.72rem; color: var(--text-muted); margin-bottom: 3px;">${t('kb.field_product')}</label>
                                <div style="display: flex; gap: 4px;">
                                    <select id="kb-new-product" style="flex: 1; background: var(--surface-lowest); border: 1px solid var(--border-subtle); color: var(--text-primary); padding: 6px; border-radius: 4px; font-size: 0.78rem;">
                                        <option value="earnest">earnest</option>
                                    </select>
                                    <button type="button" id="kb-new-product-quick" title="Créer un nouveau produit" style="background: var(--surface-lowest); border: 1px solid var(--accent-cyan); color: var(--accent-cyan); padding: 0 9px; border-radius: 4px; cursor: pointer; font-weight: 700;">+</button>
                                </div>
                            </div>
                        </div>

                        <div>
                            <label style="display: block; font-size: 0.72rem; color: var(--text-muted); margin-bottom: 3px;">${t('kb.field_title')}</label>
                            <input type="text" id="kb-new-title" placeholder="${t('kb.ph_title')}" style="width: 100%; box-sizing: border-box; background: var(--surface-lowest); border: 1px solid var(--border-subtle); color: var(--text-primary); padding: 7px; border-radius: 4px; font-size: 0.8rem;" />
                        </div>

                        <div>
                            <label style="display: block; font-size: 0.72rem; color: #fb7185; margin-bottom: 3px;">${t('kb.field_symptom')}</label>
                            <textarea id="kb-new-symptom" rows="2" placeholder="${t('kb.ph_symptom')}" style="width: 100%; box-sizing: border-box; background: var(--surface-lowest); border: 1px solid var(--border-subtle); color: var(--text-primary); padding: 6px; border-radius: 4px; font-size: 0.78rem; resize: vertical;"></textarea>
                        </div>

                        <div>
                            <label style="display: block; font-size: 0.72rem; color: var(--accent-emerald); margin-bottom: 3px;">${t('kb.field_solution')}</label>
                            <textarea id="kb-new-correction" rows="3" placeholder="${t('kb.ph_solution')}" style="width: 100%; box-sizing: border-box; background: var(--surface-lowest); border: 1px solid var(--border-subtle); color: var(--text-primary); padding: 6px; border-radius: 4px; font-size: 0.78rem; resize: vertical;"></textarea>
                        </div>

                        <div>
                            <label style="display: block; font-size: 0.72rem; color: #38bdf8; margin-bottom: 3px;">${t('kb.field_snippet')}</label>
                            <textarea id="kb-new-example" rows="2" placeholder="${t('kb.ph_snippet')}" style="width: 100%; box-sizing: border-box; background: var(--surface-lowest); border: 1px solid var(--border-subtle); color: var(--text-primary); padding: 6px; border-radius: 4px; font-size: 0.75rem; font-family: var(--font-mono); resize: vertical;"></textarea>
                        </div>

                        <div style="display: flex; justify-content: flex-end; gap: 0.6rem; margin-top: 0.5rem;">
                            <button id="kb-modal-cancel" style="background: transparent; border: 1px solid var(--border-subtle); color: var(--text-primary); padding: 6px 14px; border-radius: 4px; cursor: pointer; font-size: 0.78rem;">${t('btn.cancel')}</button>
                            <button id="kb-modal-submit" class="btn-primary" style="padding: 6px 16px; font-size: 0.78rem; font-weight: 700;">${t('kb.btn_save_seal')}</button>
                        </div>
                    </div>
                </div>

                <!-- Modal Nouveau Produit -->
                <div id="kb-modal-product" style="display: none; position: fixed; inset: 0; background: rgba(0,0,0,0.8); z-index: 10000; align-items: center; justify-content: center; backdrop-filter: blur(4px);">
                    <div style="background: var(--surface-container); border: 1px solid var(--accent-cyan); width: 420px; max-width: 90vw; border-radius: 8px; padding: 1.35rem; display: flex; flex-direction: column; gap: 0.9rem; font-family: var(--font-mono); box-shadow: var(--shadow-soft), var(--shadow-glow-cyan);">
                        <div style="display: flex; justify-content: space-between; align-items: center; border-bottom: 1px solid var(--border-subtle); padding-bottom: 0.5rem;">
                            <span style="font-weight: 800; color: var(--accent-cyan); font-size: 0.9rem;">${t('kb.prod_modal_title')}</span>
                            <button id="kb-prod-close" style="background: transparent; border: none; color: var(--text-primary); cursor: pointer; font-size: 1.1rem;">✕</button>
                        </div>
                        <div>
                            <label style="display: block; font-size: 0.72rem; color: var(--text-muted); margin-bottom: 3px;">${t('kb.prod_field_id')}</label>
                            <input type="text" id="kb-prod-id" placeholder="${t('kb.prod_ph_id')}" style="width: 100%; box-sizing: border-box; background: var(--surface-lowest); border: 1px solid var(--border-subtle); color: var(--text-primary); padding: 6px; border-radius: 4px; font-size: 0.8rem;" />
                        </div>
                        <div>
                            <label style="display: block; font-size: 0.72rem; color: var(--text-muted); margin-bottom: 3px;">${t('kb.prod_field_name')}</label>
                            <input type="text" id="kb-prod-name" placeholder="${t('kb.prod_ph_name')}" style="width: 100%; box-sizing: border-box; background: var(--surface-lowest); border: 1px solid var(--border-subtle); color: var(--text-primary); padding: 6px; border-radius: 4px; font-size: 0.8rem;" />
                        </div>
                        <div>
                            <label style="display: block; font-size: 0.72rem; color: var(--text-muted); margin-bottom: 3px;">${t('kb.prod_field_version')}</label>
                            <input type="text" id="kb-prod-version" value="1.0.0" placeholder="ex: 5.1" style="width: 100%; box-sizing: border-box; background: var(--surface-lowest); border: 1px solid var(--border-subtle); color: var(--text-primary); padding: 6px; border-radius: 4px; font-size: 0.8rem;" />
                        </div>
                        <div style="display: flex; justify-content: flex-end; gap: 0.6rem; margin-top: 0.25rem;">
                            <button id="kb-prod-cancel" style="background: transparent; border: 1px solid var(--border-subtle); color: var(--text-primary); padding: 6px 14px; border-radius: 4px; cursor: pointer; font-size: 0.78rem;">${t('btn.cancel')}</button>
                            <button id="kb-prod-submit" class="btn-primary" style="padding: 6px 16px; font-size: 0.78rem; font-weight: 700;">${t('kb.prod_btn_create')}</button>
                        </div>
                    </div>
                </div>
            </div>
        `;

        this.injectStyles();
    }

    injectStyles() {
        if (!document.getElementById('kb-styles')) {
            const style = document.createElement('style');
            style.id = 'kb-styles';
            style.textContent = `
                .kb-pill, .kb-pill-status {
                    background: var(--surface-lowest);
                    border: 1px solid var(--border-subtle);
                    color: var(--text-muted);
                    padding: 4px 10px;
                    border-radius: 14px;
                    cursor: pointer;
                    transition: all 0.15s ease;
                }
                .kb-pill:hover, .kb-pill-status:hover {
                    color: var(--text-primary);
                    border-color: var(--accent-cyan);
                }
                .kb-pill.active, .kb-pill-status.active {
                    background: rgba(128, 131, 255, 0.22);
                    border-color: var(--accent-cyan);
                    color: var(--accent-cyan);
                    font-weight: 700;
                }
                .kb-card-item {
                    background: var(--surface-low);
                    border: 1px solid var(--border-subtle);
                    border-radius: 6px;
                    padding: 0.85rem;
                    cursor: pointer;
                    transition: all 0.15s ease;
                    display: flex;
                    flex-direction: column;
                    gap: 0.4rem;
                }
                .kb-card-item:hover {
                    border-color: var(--border-accent);
                    background: var(--surface-container);
                    transform: translateY(-1px);
                }
                .kb-card-item.selected {
                    border-color: var(--accent-cyan);
                    background: rgba(128, 131, 255, 0.16);
                    box-shadow: var(--shadow-glow-cyan);
                }
            `;
            document.head.appendChild(style);
        }
    }

    bindEvents() {
        // Search
        const searchInput = this.container.querySelector('#kb-search-input');
        searchInput.addEventListener('input', (e) => {
            this.searchQuery = e.target.value.trim();
            this.filterAndRender();
        });

        // Refresh
        this.container.querySelector('#kb-btn-refresh').addEventListener('click', () => this.loadEntries());

        // Export JSON
        this.container.querySelector('#kb-btn-export').addEventListener('click', () => {
            const blob = new Blob([JSON.stringify(this.entries || [], null, 2)], { type: 'application/json' });
            const url = URL.createObjectURL(blob);
            const a = document.createElement('a');
            a.href = url;
            a.download = `earnest-bcpe-export-${new Date().toISOString().slice(0, 10)}.json`;
            a.click();
            URL.revokeObjectURL(url);
        });

        // Pill filters kind
        this.container.querySelectorAll('.kb-pill').forEach(btn => {
            btn.addEventListener('click', () => {
                this.container.querySelectorAll('.kb-pill').forEach(b => b.classList.remove('active'));
                btn.classList.add('active');
                this.currentFilterKind = btn.getAttribute('data-kind');
                this.filterAndRender();
            });
        });

        // Pill filters status
        this.container.querySelectorAll('.kb-pill-status').forEach(btn => {
            btn.addEventListener('click', () => {
                this.container.querySelectorAll('.kb-pill-status').forEach(b => b.classList.remove('active'));
                btn.classList.add('active');
                this.currentFilterStatus = btn.getAttribute('data-status');
                this.filterAndRender();
            });
        });

        // Modal Produit
        const btnNewProd = this.container.querySelector('#kb-btn-new-product');
        const btnQuickProd = this.container.querySelector('#kb-new-product-quick');
        const prodModal = this.container.querySelector('#kb-modal-product');
        const prodClose = this.container.querySelector('#kb-prod-close');
        const prodCancel = this.container.querySelector('#kb-prod-cancel');
        const prodSubmit = this.container.querySelector('#kb-prod-submit');
        const filterProd = this.container.querySelector('#kb-filter-product');

        if (filterProd) {
            filterProd.addEventListener('change', () => {
                this.currentFilterProduct = filterProd.value;
                this.filterAndRender();
            });
        }
        const openProdModal = () => {
            if (prodModal) prodModal.style.display = 'flex';
            const idInput = this.container.querySelector('#kb-prod-id');
            if (idInput) idInput.focus();
        };
        const closeProdModal = () => { if (prodModal) prodModal.style.display = 'none'; };
        if (btnNewProd) btnNewProd.addEventListener('click', openProdModal);
        if (btnQuickProd) btnQuickProd.addEventListener('click', openProdModal);
        if (prodClose) prodClose.addEventListener('click', closeProdModal);
        if (prodCancel) prodCancel.addEventListener('click', closeProdModal);

        if (prodSubmit) {
            prodSubmit.addEventListener('click', async () => {
                const product_id = this.container.querySelector('#kb-prod-id').value.trim();
                const name = this.container.querySelector('#kb-prod-name').value.trim();
                const current_version = this.container.querySelector('#kb-prod-version').value.trim();
                if (!product_id) {
                    alert(window.t ? window.t('kb.prod_err_required') : 'Product identifier is required.');
                    return;
                }
                prodSubmit.disabled = true;
                try {
                    const res = await fetch('/api/ide/kb/products', {
                        method: 'POST',
                        headers: { 'Content-Type': 'application/json' },
                        body: JSON.stringify({ product_id, name, current_version })
                    });
                    const data = await res.json();
                    if (data.status === 'CREATED') {
                        closeProdModal();
                        this.container.querySelector('#kb-prod-id').value = '';
                        this.container.querySelector('#kb-prod-name').value = '';
                        await this.loadProducts();
                        const newProdSelect = this.container.querySelector('#kb-new-product');
                        if (newProdSelect && data.product && data.product.product_id) {
                            newProdSelect.value = data.product.product_id;
                        }
                    } else {
                        alert(data.error || 'Échec création produit');
                    }
                } catch (err) {
                    alert(`Erreur réseau: ${err.message}`);
                } finally {
                    prodSubmit.disabled = false;
                }
            });
        }

        // Modal Règle / Piège
        const btnNew = this.container.querySelector('#kb-btn-new-rule');
        const modal = this.container.querySelector('#kb-modal-new');
        const btnClose = this.container.querySelector('#kb-modal-close');
        const btnCancel = this.container.querySelector('#kb-modal-cancel');
        const btnSubmit = this.container.querySelector('#kb-modal-submit');

        btnNew.addEventListener('click', () => {
            this.renderProductSelectors();
            modal.style.display = 'flex';
        });
        const closeModal = () => modal.style.display = 'none';
        btnClose.addEventListener('click', closeModal);
        btnCancel.addEventListener('click', closeModal);

        btnSubmit.addEventListener('click', async () => {
            const kind = this.container.querySelector('#kb-new-kind').value;
            const product_id = this.container.querySelector('#kb-new-product').value.trim() || 'earnest';
            const title = this.container.querySelector('#kb-new-title').value.trim();
            const symptom = this.container.querySelector('#kb-new-symptom').value.trim();
            const correction = this.container.querySelector('#kb-new-correction').value.trim();
            const example = this.container.querySelector('#kb-new-example').value.trim();

            const t = (k) => (window.t ? window.t(k) : k);
            if (!title || !correction) {
                alert(t('kb.err_required'));
                return;
            }

            // Déduplication : vérifier si un titre similaire existe déjà
            const normTitle = title.toLowerCase();
            const duplicate = this.entries.find(e => e.title && e.title.toLowerCase() === normTitle);
            if (duplicate && !confirm(t('kb.confirm_dup').replace('{title}', duplicate.title))) {
                return;
            }

            btnSubmit.disabled = true;
            btnSubmit.textContent = t('kb.saving');

            try {
                const res = await fetch('/api/ide/kb/create', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify({ product_id, kind, title, symptom, correction, example, author: 'human_expert' })
                });
                const data = await res.json();
                if (data.status === 'CREATED') {
                    closeModal();
                    this.container.querySelector('#kb-new-title').value = '';
                    this.container.querySelector('#kb-new-symptom').value = '';
                    this.container.querySelector('#kb-new-correction').value = '';
                    this.container.querySelector('#kb-new-example').value = '';
                    await this.loadEntries();
                    this.selectedEntryId = data.entry_id;
                    this.filterAndRender();
                } else {
                    alert(`${t('kb.err_prefix')} ${data.error || 'Failed'}`);
                }
            } catch (err) {
                alert(`${t('kb.err_net')} ${err.message}`);
            } finally {
                btnSubmit.disabled = false;
                btnSubmit.textContent = window.t ? window.t('kb.btn_save_seal') : "Save & Seal";
            }
        });
    }

    async loadProducts() {
        try {
            const res = await fetch('/api/ide/kb/products');
            const data = await res.json();
            this.products = Array.isArray(data.products) ? data.products : [];
            this.renderProductSelectors();
        } catch (err) { }
    }

    renderProductSelectors() {
        const filterSel = this.container.querySelector('#kb-filter-product');
        const newSel = this.container.querySelector('#kb-new-product');
        const list = this.products.length > 0 ? this.products : [{ product_id: 'earnest', name: 'earnest', entry_count: 0 }];

        if (filterSel) {
            const prev = this.currentFilterProduct || '';
            const allLabel = window.t ? window.t('kb.filter_all_products') : 'All products';
            filterSel.innerHTML = `<option value="">${allLabel}</option>`;
            list.forEach(p => {
                const opt = document.createElement('option');
                opt.value = p.product_id;
                opt.textContent = `${p.product_id} (${p.entry_count || 0})`;
                filterSel.appendChild(opt);
            });
            filterSel.value = prev;
        }
        if (newSel) {
            const prevNew = newSel.value || 'earnest';
            newSel.innerHTML = '';
            list.forEach(p => {
                const opt = document.createElement('option');
                opt.value = p.product_id;
                opt.textContent = p.name && p.name !== p.product_id ? `${p.product_id} — ${p.name}` : p.product_id;
                newSel.appendChild(opt);
            });
            if ([...newSel.options].some(o => o.value === prevNew)) {
                newSel.value = prevNew;
            }
        }
    }

    async loadEntries() {
        try {
            await this.loadProducts();
            const res = await fetch('/api/ide/kb/entries');
            const data = await res.json();
            this.entries = data.entries || [];
            const badge = this.container.querySelector('#kb-count-badge');
            if (badge) badge.textContent = `${this.entries.length} ${window.t ? window.t('kb.truths') : 'truths'}`;

            if (this.entries.length > 0 && !this.selectedEntryId) {
                this.selectedEntryId = this.entries[0].entry_id;
            }

            this.filterAndRender();
        } catch (err) {
            const stream = this.container.querySelector('#kb-cards-stream');
            const t = (k) => (window.t ? window.t(k) : k);
            stream.innerHTML = `<div style="color: var(--accent-rose); font-size: 0.8rem; padding: 2rem; text-align: center;">${t('kb.err_load')} ${err.message}</div>`;
        }
    }

    filterAndRender() {
        const stream = this.container.querySelector('#kb-cards-stream');
        let filtered = this.entries;

        if (this.currentFilterKind) {
            filtered = filtered.filter(e => e.kind === this.currentFilterKind);
        }
        if (this.currentFilterStatus) {
            filtered = filtered.filter(e => e.status === this.currentFilterStatus);
        }
        if (this.currentFilterProduct) {
            filtered = filtered.filter(e => e.product_id === this.currentFilterProduct);
        }
        if (this.searchQuery) {
            const q = this.searchQuery.toLowerCase();
            filtered = filtered.filter(e =>
                (e.title && e.title.toLowerCase().includes(q)) ||
                (e.symptom && e.symptom.toLowerCase().includes(q)) ||
                (e.cause && e.cause.toLowerCase().includes(q)) ||
                (e.correction && e.correction.toLowerCase().includes(q))
            );
        }

        const t = (k) => (window.t ? window.t(k) : k);
        if (filtered.length === 0) {
            stream.innerHTML = `<div style="text-align: center; color: var(--text-muted); font-size: 0.8rem; padding: 3rem;">${t('kb.empty_filter')}</div>`;
            this.renderDetail(null);
            return;
        }

        stream.innerHTML = filtered.map(item => this.renderListItem(item)).join('');

        // Clic sur une carte pour afficher le détail à droite
        stream.querySelectorAll('.kb-card-item').forEach(el => {
            el.addEventListener('click', () => {
                const id = parseInt(el.getAttribute('data-id'), 10);
                this.selectedEntryId = id;
                stream.querySelectorAll('.kb-card-item').forEach(c => c.classList.remove('selected'));
                el.classList.add('selected');
                const found = this.entries.find(e => e.entry_id === id);
                this.renderDetail(found);
            });
        });

        // Sélectionner par défaut l'entrée active
        const selected = this.entries.find(e => e.entry_id === this.selectedEntryId) || filtered[0];
        if (selected) {
            this.selectedEntryId = selected.entry_id;
            const targetEl = stream.querySelector(`[data-id="${selected.entry_id}"]`);
            if (targetEl) targetEl.classList.add('selected');
            this.renderDetail(selected);
        }
    }

    renderListItem(item) {
        const t = (k) => (window.t ? window.t(k) : k);
        const kindBadge = {
            'piege': `<span style="background: rgba(244, 63, 94, 0.2); color: #fb7185; border: 1px solid rgba(244, 63, 94, 0.4); padding: 1px 6px; border-radius: 4px; font-size: 0.65rem; font-family: var(--font-mono);">${t('kb.badge_pitfall')}</span>`,
            'regle': `<span style="background: rgba(16, 185, 129, 0.2); color: #34d399; border: 1px solid rgba(16, 185, 129, 0.4); padding: 1px 6px; border-radius: 4px; font-size: 0.65rem; font-family: var(--font-mono);">${t('kb.badge_rule')}</span>`,
            'catalogue': `<span style="background: rgba(56, 189, 248, 0.2); color: #38bdf8; border: 1px solid rgba(56, 189, 248, 0.4); padding: 1px 6px; border-radius: 4px; font-size: 0.65rem; font-family: var(--font-mono);">${t('kb.badge_catalog')}</span>`
        }[item.kind] || `<span>${item.kind}</span>`;

        const statusIcon = item.status === 'valide'
            ? `<span style="color: #34d399; font-size: 0.7rem;">${t('kb.card_sealed')}</span>`
            : `<span style="color: #facc15; font-size: 0.7rem;">${t('kb.card_discovered')}</span>`;

        return `
            <div class="kb-card-item ${item.entry_id === this.selectedEntryId ? 'selected' : ''}" data-id="${item.entry_id}">
                <div style="display: flex; justify-content: space-between; align-items: center;">
                    ${kindBadge}
                    <div style="font-family: var(--font-mono); font-size: 0.68rem;">${statusIcon}</div>
                </div>
                <div style="font-weight: 700; font-size: 0.82rem; color: var(--text-primary); line-height: 1.35; margin-top: 2px;">
                    ${item.title}
                </div>
                <div style="display: flex; justify-content: space-between; align-items: center; font-size: 0.68rem; color: var(--text-muted); font-family: var(--font-mono); margin-top: 4px;">
                    <span>#${item.entry_id} • ${item.product_id}</span>
                    <span>${t('kb.card_hits')} <strong style="color: #34d399;">${item.use_count}x</strong></span>
                </div>
            </div>
        `;
    }

    renderDetail(item) {
        const t = (k) => (window.t ? window.t(k) : k);
        const detailPane = this.container.querySelector('#kb-detail-pane');
        if (!item) {
            detailPane.innerHTML = `
                <div style="text-align: center; color: var(--text-muted); font-size: 0.85rem; padding: 4rem;">
                    ${t('kb.no_selection')}
                </div>
            `;
            return;
        }

        const isValide = item.status === 'valide';
        const kindBadge = {
            'piege': `<span style="background: rgba(244, 63, 94, 0.2); color: #fb7185; border: 1px solid rgba(244, 63, 94, 0.4); padding: 3px 10px; border-radius: 4px; font-size: 0.75rem; font-family: var(--font-mono); font-weight: 700;">${t('kb.badge_pitfall_full')}</span>`,
            'regle': `<span style="background: rgba(16, 185, 129, 0.2); color: #34d399; border: 1px solid rgba(16, 185, 129, 0.4); padding: 3px 10px; border-radius: 4px; font-size: 0.75rem; font-family: var(--font-mono); font-weight: 700;">${t('kb.badge_rule_full')}</span>`,
            'catalogue': `<span style="background: rgba(56, 189, 248, 0.2); color: #38bdf8; border: 1px solid rgba(56, 189, 248, 0.4); padding: 3px 10px; border-radius: 4px; font-size: 0.75rem; font-family: var(--font-mono); font-weight: 700;">${t('kb.badge_catalog_full')}</span>`
        }[item.kind] || `<span>${item.kind}</span>`;

        detailPane.innerHTML = `
            <div style="display: flex; justify-content: space-between; align-items: flex-start; border-bottom: 1px solid var(--border-subtle); padding-bottom: 1rem;">
                <div style="display: flex; flex-direction: column; gap: 0.4rem; max-width: 70%;">
                    <div style="display: flex; align-items: center; gap: 0.6rem;">
                        ${kindBadge}
                        <span style="font-size: 0.72rem; color: var(--text-muted); font-family: var(--font-mono);">ID #${item.entry_id} • ${t('kb.detail_key')} <code>${item.subject_key}</code></span>
                    </div>
                    <div style="font-size: 1.2rem; font-weight: 800; color: var(--text-primary); line-height: 1.3;">
                        ${item.title}
                    </div>
                </div>

                <div style="display: flex; flex-direction: column; align-items: flex-end; gap: 0.4rem;">
                    ${isValide ? `
                        <div style="display: flex; align-items: center; gap: 5px; color: #34d399; font-size: 0.78rem; font-family: var(--font-mono); background: rgba(16, 185, 129, 0.15); border: 1px solid rgba(16, 185, 129, 0.3); padding: 4px 10px; border-radius: 6px;">
                            <span>✓</span> ${t('kb.status_validated_by')} <strong>${item.validated_by || 'human'}</strong>
                        </div>
                        <button id="kb-btn-revisit" class="icon-btn" style="border: 1px solid var(--border-subtle); padding: 4px 10px; font-size: 0.72rem; border-radius: 4px; color: var(--accent-amber);">
                            ${t('kb.btn_revisit')}
                        </button>
                    ` : `
                        <div style="color: #facc15; font-size: 0.78rem; font-family: var(--font-mono); background: rgba(234, 179, 8, 0.15); border: 1px solid rgba(234, 179, 8, 0.3); padding: 4px 10px; border-radius: 6px;">
                            ${t('kb.status_unsealed')}
                        </div>
                        <button id="kb-btn-validate-seal" class="btn-primary" style="padding: 6px 14px; font-size: 0.78rem; font-weight: 700; background: #10b981; border-color: #059669;">
                            ${t('kb.btn_validate_seal')}
                        </button>
                    `}
                </div>
            </div>

            <!-- Stats & Audit -->
            <div style="display: grid; grid-template-columns: repeat(4, 1fr); gap: 0.75rem; background: var(--surface-low); padding: 0.75rem; border-radius: 6px; border: 1px solid var(--border-subtle); font-family: var(--font-mono); font-size: 0.72rem;">
                <div><span style="color: var(--text-muted);">${t('kb.stat_component')}</span> <strong style="color: var(--accent-cyan);">${item.product_id}</strong></div>
                <div><span style="color: var(--text-muted);">${t('kb.stat_reuses')}</span> <strong style="color: #34d399;">${item.use_count} ${t('kb.stat_times')}</strong></div>
                <div><span style="color: var(--text-muted);">${t('kb.stat_prevented')}</span> <strong style="color: #fb7185;">${item.fail_count}</strong></div>
                <div><span style="color: var(--text-muted);">${t('kb.stat_origin')}</span> <strong>${item.source}</strong></div>
            </div>

            <!-- Section 1 : Symptôme & Cause -->
            ${item.symptom ? `
                <div style="background: rgba(244, 63, 94, 0.08); border: 1px solid rgba(244, 63, 94, 0.3); border-left: 4px solid #fb7185; border-radius: 6px; padding: 1rem; display: flex; flex-direction: column; gap: 0.4rem;">
                    <div style="font-weight: 800; font-family: var(--font-mono); color: #fb7185; font-size: 0.8rem; display: flex; align-items: center; gap: 6px;">
                        <span>💥</span> ${t('kb.sec_symptom')}
                    </div>
                    <div style="color: #fecdd3; font-size: 0.85rem; line-height: 1.5;">${item.symptom}</div>
                    ${item.cause ? `<div style="font-size: 0.78rem; color: #fda4af; margin-top: 4px;"><strong>${t('kb.root_cause')}</strong> ${item.cause}</div>` : ''}
                </div>
            ` : ''}

            <!-- Section 2 : Solution Obligatoire -->
            <div style="background: rgba(16, 185, 129, 0.08); border: 1px solid rgba(16, 185, 129, 0.3); border-left: 4px solid #34d399; border-radius: 6px; padding: 1rem; display: flex; flex-direction: column; gap: 0.4rem;">
                <div style="font-weight: 800; font-family: var(--font-mono); color: #34d399; font-size: 0.8rem; display: flex; align-items: center; gap: 6px;">
                    <span>🛡️</span> ${t('kb.sec_solution')}
                </div>
                <div style="color: #d1fae5; font-size: 0.88rem; line-height: 1.5; font-weight: 500;">
                    ${item.correction}
                </div>
            </div>

            <!-- Section 3 : Exemple de Code -->
            ${item.example ? `
                <div style="background: var(--surface-lowest); border: 1px solid var(--border-subtle); border-radius: 6px; padding: 1rem; display: flex; flex-direction: column; gap: 0.5rem;">
                    <div style="font-family: var(--font-mono); font-size: 0.75rem; color: var(--accent-cyan); font-weight: 700; display: flex; justify-content: space-between; align-items: center;">
                        <span>💻 ${t('kb.sec_snippet')}</span>
                        <button id="kb-btn-copy-code" class="icon-btn" style="font-size: 0.7rem; border: 1px solid var(--border-subtle); padding: 2px 8px; border-radius: 4px;">${t('kb.btn_copy')}</button>
                    </div>
                    <pre style="margin: 0; font-family: var(--font-mono); font-size: 0.8rem; color: #38bdf8; overflow-x: auto; white-space: pre-wrap; line-height: 1.45;">${item.example}</pre>
                </div>
            ` : ''}
        `;

        // Événements du panneau de détail
        const btnValidate = detailPane.querySelector('#kb-btn-validate-seal');
        if (btnValidate) {
            btnValidate.addEventListener('click', async () => {
                btnValidate.disabled = true;
                btnValidate.textContent = t('kb.sealing');
                await this.annotateEntry(item.entry_id, 'valide');
            });
        }

        const btnRevisit = detailPane.querySelector('#kb-btn-revisit');
        if (btnRevisit) {
            btnRevisit.addEventListener('click', async () => {
                await this.annotateEntry(item.entry_id, 'a_revoir');
            });
        }

        const btnCopy = detailPane.querySelector('#kb-btn-copy-code');
        if (btnCopy && item.example) {
            btnCopy.addEventListener('click', () => {
                navigator.clipboard.writeText(item.example);
                btnCopy.textContent = t('kb.btn_copied');
                setTimeout(() => { btnCopy.textContent = t('kb.btn_copy'); }, 2000);
            });
        }
    }

    async annotateEntry(id, newStatus) {
        const t = (k) => (window.t ? window.t(k) : k);
        try {
            const res = await fetch('/api/ide/kb/annotate', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ entry_id: id, status: newStatus, annotator: 'human_expert' })
            });
            const data = await res.json();
            if (data.status === 'OK') {
                await this.loadEntries();
            } else {
                alert(`${t('kb.err_annotate')} ${data.error || 'Unknown'}`);
            }
        } catch (err) {
            alert(`${t('kb.err_net')} ${err.message}`);
        }
    }
}
