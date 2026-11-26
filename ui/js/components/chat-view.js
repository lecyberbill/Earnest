import { store } from '../store.js';
import { renderMarkdown } from '../markdown.js';
import './agent-mascot.js';

export class ChatView extends HTMLElement {
    constructor() {
        super();
        this.messages = [];
        this.sessionId = null;
        this.isListening = false;
        this.recognition = null;
    }

    connectedCallback() {
        if (this._initialized) return;
        this._initialized = true;

        this.innerHTML = `
            <div class="card" style="height: calc(100vh - 120px); display: flex; flex-direction: column; margin-bottom: 0; padding: 1.5rem;">
                <div style="display: flex; justify-content: space-between; align-items: center; gap: 0.75rem; flex-wrap: wrap; border-bottom: 1px solid var(--border-subtle); padding-bottom: 0.85rem; margin-bottom: 1rem;">
                    <div style="display: flex; align-items: center; gap: 0.85rem;">
                        <agent-mascot id="chat-mascot"></agent-mascot>
                        <div>
                            <h2 style="display: flex; align-items: center; gap: 0.5rem; margin: 0;">
                                <span data-i18n="chat.title">Salon de Discussion Directe</span>
                                <span id="psy-status-badge" data-i18n="chat.status_listening" style="font-size: 0.7rem; font-family: var(--font-mono); color: var(--accent-cyan); background: rgba(34, 211, 238, 0.1); border: 1px solid rgba(34, 211, 238, 0.3); padding: 0.15rem 0.45rem; border-radius: 999px;">À l'écoute</span>
                            </h2>
                            <p class="card-desc" data-i18n="chat.desc" style="margin: 0.2rem 0 0 0;">Discussion continue persistée • Choisis ton modèle et discute avec Dr. Earnest.</p>
                        </div>
                    </div>
                    <div style="display: flex; gap: 0.5rem; align-items: center; flex-wrap: wrap;">
                        <select id="chat-backend-select" title="Modèle / Moteur de discussion" style="background: rgba(15, 23, 42, 0.85); border: 1px solid var(--border-subtle); color: var(--accent-cyan); border-radius: var(--radius-sm); padding: 0.35rem 0.6rem; font-size: 0.78rem; font-family: var(--font-mono); max-width: 220px;">
                            <option value="auto" data-i18n="chat.model_auto">Modèle : Auto (Priorité)</option>
                        </select>
                        <select id="chat-effort-select" title="Niveau de réflexion" style="background: rgba(15, 23, 42, 0.85); border: 1px solid var(--border-subtle); color: #a5b4fc; border-radius: var(--radius-sm); padding: 0.35rem 0.55rem; font-size: 0.78rem; font-family: var(--font-mono);">
                            <option value="off" selected>Effort : Off</option>
                            <option value="low">Effort : Low</option>
                            <option value="medium">Effort : Medium</option>
                            <option value="high">Effort : High</option>
                        </select>
                        <select id="chat-session-select" style="background: rgba(15, 23, 42, 0.8); border: 1px solid var(--border-subtle); color: var(--text-primary); border-radius: var(--radius-sm); padding: 0.35rem 0.6rem; font-size: 0.78rem; font-family: var(--font-mono); max-width: 180px;">
                            <option value="" data-i18n="chat.new_session">Nouvelle Session</option>
                        </select>
                        <button id="btn-clear-chat" data-i18n="chat.new_thread" style="background: rgba(255,255,255,0.05); border: 1px solid var(--border-subtle); color: var(--text-muted); padding: 0.4rem 0.85rem; border-radius: var(--radius-sm); cursor: pointer; font-size: 0.8rem;">
                            Nouveau Fil
                        </button>
                    </div>
                </div>

                <!-- Zone d'affichage des messages -->
                <div id="chat-stream" style="flex: 1; overflow-y: auto; display: flex; flex-direction: column; gap: 0.85rem; padding-right: 0.5rem;">
                    <div class="chat-bubble assistant" data-i18n="chat.welcome">
                        Bonjour ! Je suis Earnest. Tu peux me poser des questions, me demander de réfléchir à un sujet ou discuter de ton projet librement ici.
                    </div>
                </div>

                <!-- Input d'envoi avec Micro et Action -->
                <div style="display: flex; gap: 0.65rem; margin-top: 1rem; padding-top: 0.85rem; border-top: 1px solid var(--border-subtle); align-items: center;">
                    <input type="text" id="chat-input" data-i18n-placeholder="chat.input_placeholder" placeholder="Écrivez ou dictez votre message (Entrée pour envoyer)..." style="flex: 1;" />
                    
                    <button id="btn-chat-mic" class="btn-attach" style="cursor: pointer;" title="Dictée Vocale (Web Speech API)">
                        <span id="mic-icon">
                            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 1a3 3 0 0 0-3 3v8a3 3 0 0 0 6 0V4a3 3 0 0 0-3-3z"></path><path d="M19 10v1a7 7 0 0 1-14 0v-1"></path><line x1="12" y1="19" x2="12" y2="23"></line><line x1="8" y1="23" x2="16" y2="23"></line></svg>
                        </span>
                    </button>

                    <button class="btn-primary" id="btn-send-chat" style="display: inline-flex; align-items: center; gap: 0.35rem;">
                        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="22" y1="2" x2="11" y2="13"></line><polygon points="22 2 15 22 11 13 2 9 22 2"></polygon></svg>
                        Envoyer
                    </button>
                </div>
            </div>
        `;

        this.initVoiceRecognition();
        this.loadProviders();
        this.loadSessions();
        this.psyState = 'idle';
        this.startPsyBotAnimation();

        this.querySelector('#btn-send-chat').addEventListener('click', () => this.sendMessage());
        this.querySelector('#chat-input').addEventListener('keydown', (e) => {
            if (e.key === 'Enter') this.sendMessage();
        });
        this.querySelector('#btn-clear-chat').addEventListener('click', () => this.startNewSession());
        this.querySelector('#chat-session-select').addEventListener('change', (e) => this.switchSession(e.target.value));
        this.querySelector('#btn-chat-mic').addEventListener('click', () => this.toggleVoiceDictation());
    }

    initVoiceRecognition() {
        this.mediaRecorder = null;
        this.audioChunks = [];
    }

    async toggleVoiceDictation() {
        const btn = this.querySelector('#btn-chat-mic');
        const icon = this.querySelector('#mic-icon');
        const input = this.querySelector('#chat-input');

        if (!this.isListening) {
            try {
                const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
                this.audioChunks = [];
                this.mediaRecorder = new MediaRecorder(stream);
                this.mediaRecorder.ondataavailable = (e) => {
                    if (e.data.size > 0) this.audioChunks.push(e.data);
                };
                this.mediaRecorder.onstop = async () => {
                    stream.getTracks().forEach(t => t.stop());
                    const audioBlob = new Blob(this.audioChunks, { type: 'audio/webm' });
                    icon.innerHTML = `<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" style="animation: spin 1s linear infinite;"><path d="M21.5 2v6h-6M21.34 15.57a10 10 0 1 1-.57-8.38l5.67-5.67"/></svg>`;
                    btn.style.borderColor = 'var(--accent-amber)';

                    const reader = new FileReader();
                    reader.onloadend = async () => {
                        try {
                            const res = await fetch('/api/transcribe', {
                                method: 'POST',
                                headers: { 'Content-Type': 'application/json' },
                                body: JSON.stringify({ audio_base64: reader.result })
                            });
                            const data = await res.json();
                            if (data.status === 'ok' && data.text) {
                                input.value = (input.value ? input.value + ' ' : '') + data.text;
                            } else if (data.error) {
                                alert(`Erreur Whisper: ${data.error}`);
                            }
                        } catch (err) {
                            alert(`Erreur réseau transcription: ${err.message}`);
                        } finally {
                            icon.innerHTML = `<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 1a3 3 0 0 0-3 3v8a3 3 0 0 0 6 0V4a3 3 0 0 0-3-3z"></path><path d="M19 10v1a7 7 0 0 1-14 0v-1"></path><line x1="12" y1="19" x2="12" y2="23"></line><line x1="8" y1="23" x2="16" y2="23"></line></svg>`;
                            btn.style.borderColor = 'var(--border-subtle)';
                        }
                    };
                    reader.readAsDataURL(audioBlob);
                };

                this.mediaRecorder.start();
                this.isListening = true;
                btn.style.borderColor = 'var(--accent-rose)';
                icon.innerHTML = `<span style="display:inline-block; width:10px; height:10px; border-radius:50%; background:var(--accent-rose); animation: pulse 1s infinite;"></span>`;
            } catch (err) {
                alert(`Accès microphone impossible : ${err.name} - ${err.message}`);
                this.stopListening();
            }
        } else {
            if (this.mediaRecorder && this.mediaRecorder.state !== 'inactive') {
                this.mediaRecorder.stop();
            }
            this.isListening = false;
        }
    }

    stopListening() {
        this.isListening = false;
        const btn = this.querySelector('#btn-chat-mic');
        const icon = this.querySelector('#mic-icon');
        if (btn) btn.style.borderColor = 'var(--border-subtle)';
        if (icon) icon.innerHTML = `<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 1a3 3 0 0 0-3 3v8a3 3 0 0 0 6 0V4a3 3 0 0 0-3-3z"></path><path d="M19 10v1a7 7 0 0 1-14 0v-1"></path><line x1="12" y1="19" x2="12" y2="23"></line><line x1="8" y1="23" x2="16" y2="23"></line></svg>`;
    }

    async loadSessions() {
        const select = this.querySelector('#chat-session-select');
        try {
            const res = await fetch('/api/chat/sessions?project_id=default_workspace');
            const sessions = await res.json();
            select.innerHTML = '<option value="">+ Nouvelle Session</option>';
            sessions.forEach(s => {
                const opt = document.createElement('option');
                opt.value = s.id;
                opt.textContent = `${s.title.substring(0, 25)}... (${s.updated_at.split(' ')[1] || ''})`;
                select.appendChild(opt);
            });
        } catch (e) {
            console.error('Error loading sessions', e);
        }
    }

    async switchSession(sessionId) {
        if (!sessionId) {
            this.startNewSession();
            return;
        }
        this.sessionId = sessionId;
        this.messages = [];
        const stream = this.querySelector('#chat-stream');
        stream.innerHTML = '';
        try {
            const res = await fetch(`/api/chat/messages?session_id=${sessionId}`);
            const history = await res.json();
            history.forEach(m => {
                this.messages.push({ role: m.role, content: m.content });
                this.renderBubble(m.role, m.content);
            });
        } catch (e) {
            stream.innerHTML = `<div class="chat-bubble assistant error">Erreur chargement session: ${e.message}</div>`;
        }
    }

    startNewSession() {
        this.sessionId = null;
        this.messages = [];
        const stream = this.querySelector('#chat-stream');
        stream.innerHTML = `
            <div class="chat-bubble assistant">
                Nouvelle conversation démarrée. De quoi souhaites-tu parler ?
            </div>
        `;
        const select = this.querySelector('#chat-session-select');
        if (select) select.value = '';
    }

    async sendMessage() {
        const input = this.querySelector('#chat-input');
        const text = input.value.trim();
        if (!text) return;

        input.value = '';

        // Message utilisateur
        this.renderBubble('user', text);
        this.messages.push({ role: 'user', content: text });

        // Placeholder temporaire assistant
        const thinkingBubble = this.appendPlaceholder('Dr. Earnest ajuste ses lunettes et réfléchit...');
        this.setPsyState('thinking');
        this.querySelector('#chat-mascot')?.setState('think');

        try {
            const backendSel = this.querySelector('#chat-backend-select');
            const effortSel = this.querySelector('#chat-effort-select');
            const res = await fetch('/api/chat', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({
                    messages: this.messages,
                    session_id: this.sessionId,
                    project_id: 'default_workspace',
                    backend: backendSel ? backendSel.value : 'auto',
                    think_effort: effortSel ? effortSel.value : 'off'
                })
            });

            if (!res.ok) {
                const err = await res.json();
                thinkingBubble.className = 'chat-bubble assistant error';
                thinkingBubble.textContent = `Erreur: ${err.error || 'Erreur réseau'}`;
                this.setPsyState('idle');
                this.querySelector('#chat-mascot')?.setState('sleep');
                return;
            }

            const data = await res.json();
            this.sessionId = data.session_id || this.sessionId;
            const reply = data.content || 'Pas de réponse reçue.';
            thinkingBubble.remove();
            this.renderBubble('assistant', reply);
            this.messages.push({ role: 'assistant', content: reply });
            this.setPsyState('nod');
            this.querySelector('#chat-mascot')?.setState('work');
            setTimeout(() => {
                this.setPsyState('idle');
                this.querySelector('#chat-mascot')?.setState('sleep');
            }, 1400);
            this.loadSessions();
        } catch (e) {
            thinkingBubble.className = 'chat-bubble assistant error';
            thinkingBubble.textContent = `Erreur de connexion : ${e.message}`;
            this.setPsyState('idle');
            this.querySelector('#chat-mascot')?.setState('sleep');
        }
    }

    appendPlaceholder(text) {
        const stream = this.querySelector('#chat-stream');
        const bubble = document.createElement('div');
        bubble.className = 'chat-bubble assistant thinking';
        bubble.textContent = text;
        stream.appendChild(bubble);
        stream.scrollTop = stream.scrollHeight;
        return bubble;
    }

    renderBubble(role, content) {
        const stream = this.querySelector('#chat-stream');
        const bubble = document.createElement('div');
        bubble.className = `chat-bubble ${role}`;

        if (role === 'assistant') {
            bubble.innerHTML = `
                <div>${renderMarkdown(content)}</div>
                <div style="display: flex; gap: 0.6rem; margin-top: 0.5rem; padding-top: 0.4rem; border-top: 1px solid rgba(255,255,255,0.06); font-size: 0.75rem;">
                    <button class="btn-bubble-action btn-speak" title="Écouter la réponse (TTS)" style="display: inline-flex; align-items: center; gap: 0.35rem;">
                        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polygon points="11 5 6 9 2 9 2 15 6 15 11 19 11 5"></polygon><path d="M19.07 4.93a10 10 0 0 1 0 14.14M15.54 8.46a5 5 0 0 1 0 7.07"></path></svg>
                        Lire
                    </button>
                    <button class="btn-bubble-action btn-send-to-run" title="Transférer en mission dans la Console de Run" style="display: inline-flex; align-items: center; gap: 0.35rem;">
                        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polygon points="5 3 19 12 5 21 5 3"></polygon></svg>
                        Envoyer au Run
                    </button>
                </div>
            `;
            const speakBtn = bubble.querySelector('.btn-speak');
            speakBtn.addEventListener('click', () => this.speakText(content, speakBtn));
            bubble.querySelector('.btn-send-to-run').addEventListener('click', () => this.transferToRunConsole(content));
        } else {
            bubble.textContent = content;
        }

        stream.appendChild(bubble);
        stream.scrollTop = stream.scrollHeight;
    }

    _resetActiveSpeakBtn() {
        if (!this._activeSpeakBtn) return;
        const btn = this._activeSpeakBtn;
        if (this._activeSpeakBtnOriginalHtml !== undefined) {
            btn.innerHTML = this._activeSpeakBtnOriginalHtml;
        }
        btn.title = 'Écouter la réponse (TTS)';
        btn.style.color = '';
        btn.style.borderColor = '';
        btn.classList.remove('is-speaking');
        this._activeSpeakBtn = null;
        this._activeSpeakBtnOriginalHtml = undefined;
    }

    speakText(text, btn = null) {
        if (!('speechSynthesis' in window)) {
            alert('La synthèse vocale n\'est pas disponible sur ce navigateur.');
            return;
        }

        // Toggle: clicking the currently speaking button stops playback.
        if (btn && this._activeSpeakBtn === btn) {
            window.speechSynthesis.cancel();
            this._resetActiveSpeakBtn();
            return;
        }

        window.speechSynthesis.cancel();
        this._resetActiveSpeakBtn();

        const cleanText = text.replace(/[*#`_\[\]]/g, '').trim();
        if (!cleanText) return;

        const utterance = new SpeechSynthesisUtterance(cleanText);
        utterance.lang = 'fr-FR';
        utterance.rate = 1.05;

        if (btn) {
            this._activeSpeakBtn = btn;
            this._activeSpeakBtnOriginalHtml = btn.innerHTML;
            btn.innerHTML = `
                <svg width="12" height="12" viewBox="0 0 24 24" fill="currentColor" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="6" y="6" width="12" height="12" rx="1"></rect></svg>
                Stop
            `;
            btn.title = 'Arrêter la lecture (TTS)';
            btn.style.color = '#f87171';
            btn.style.borderColor = 'rgba(248, 113, 113, 0.45)';
            btn.classList.add('is-speaking');

            const onFinish = () => {
                if (this._activeSpeakBtn === btn) {
                    this._resetActiveSpeakBtn();
                }
            };
            utterance.onend = onFinish;
            utterance.onerror = onFinish;
        }

        window.speechSynthesis.speak(utterance);
    }

    transferToRunConsole(text) {
        const cleanPrompt = text.split('\n')[0].substring(0, 140);
        const promptInput = document.querySelector('run-console #prompt-input');
        if (promptInput) {
            promptInput.value = cleanPrompt;
        }
        // Basculer vers la vue run
        const runBtn = document.querySelector('.sidebar nav button[data-view="run"]');
        if (runBtn) runBtn.click();
    }

    async loadProviders() {
        const select = this.querySelector('#chat-backend-select');
        if (!select) return;
        try {
            const res = await fetch('/api/model_providers');
            const list = await res.json();
            if (!Array.isArray(list)) return;
            select.innerHTML = '<option value="auto">Modèle : Auto (Priorité)</option>';
            list.filter(p => p.is_enabled).forEach(p => {
                const opt = document.createElement('option');
                opt.value = p.id;
                opt.textContent = `${p.name} (${p.model})`;
                select.appendChild(opt);
            });
        } catch (_) {}
    }

    setPsyState(state) {
        this.psyState = state;
        const badge = this.querySelector('#psy-status-badge');
        if (!badge) return;
        if (state === 'thinking') {
            badge.textContent = 'Prend des notes...';
            badge.style.color = '#fbbf24';
            badge.style.borderColor = 'rgba(251, 191, 36, 0.4)';
            badge.style.background = 'rgba(251, 191, 36, 0.12)';
        } else if (state === 'nod') {
            badge.textContent = 'Hoche la tête';
            badge.style.color = '#34d399';
            badge.style.borderColor = 'rgba(52, 211, 153, 0.4)';
            badge.style.background = 'rgba(52, 211, 153, 0.12)';
        } else {
            badge.textContent = 'À l\'écoute';
            badge.style.color = 'var(--accent-cyan)';
            badge.style.borderColor = 'rgba(34, 211, 238, 0.3)';
            badge.style.background = 'rgba(34, 211, 238, 0.1)';
        }
    }

    startPsyBotAnimation() {
        const canvas = this.querySelector('#psy-bot-canvas');
        if (!canvas) return;
        const ctx = canvas.getContext('2d');
        let tick = 0;

        const draw = () => {
            if (!this.isConnected) return;
            tick++;
            ctx.clearRect(0, 0, 56, 56);

            const nodOffset = this.psyState === 'nod'
                ? Math.round(Math.sin(tick * 0.45) * 2)
                : (this.psyState === 'thinking' ? Math.round(Math.sin(tick * 0.18)) : (tick % 40 < 20 ? 0 : 1));
            const blink = (this.psyState === 'idle' && (tick % 90 > 85));

            // Antenne psy (petite ampoule douce)
            ctx.fillStyle = '#94a3b8';
            ctx.fillRect(27, 4 + nodOffset, 2, 4);
            ctx.fillStyle = this.psyState === 'thinking' ? '#fbbf24' : '#38bdf8';
            ctx.fillRect(26, 2 + nodOffset, 4, 3);

            // Tête du robot
            ctx.fillStyle = '#334155';
            ctx.fillRect(14, 8 + nodOffset, 28, 22);
            ctx.fillStyle = '#475569';
            ctx.fillRect(16, 10 + nodOffset, 24, 18);

            // Visière / visage
            ctx.fillStyle = '#0f172a';
            ctx.fillRect(17, 13 + nodOffset, 22, 11);

            // Yeux derrière les lunettes
            ctx.fillStyle = this.psyState === 'thinking' ? '#fbbf24' : '#22d3ee';
            if (blink) {
                ctx.fillRect(20, 18 + nodOffset, 5, 1);
                ctx.fillRect(31, 18 + nodOffset, 5, 1);
            } else {
                const pupilShift = this.psyState === 'thinking' ? (tick % 30 < 15 ? 1 : -1) : 0;
                ctx.fillRect(21 + pupilShift, 16 + nodOffset, 4, 4);
                ctx.fillRect(32 + pupilShift, 16 + nodOffset, 4, 4);
            }

            // Lunettes rondes dorées façon psy (monture + pont)
            ctx.strokeStyle = '#f59e0b';
            ctx.lineWidth = 1.5;
            ctx.strokeRect(18.5, 14.5 + nodOffset, 8, 7);
            ctx.strokeRect(29.5, 14.5 + nodOffset, 8, 7);
            ctx.fillStyle = '#f59e0b';
            ctx.fillRect(26.5, 17 + nodOffset, 3, 1.5);
            // Reflet sur les verres
            if (this.psyState === 'thinking' && (tick % 20 < 10)) {
                ctx.fillStyle = 'rgba(255,255,255,0.45)';
                ctx.fillRect(20, 15 + nodOffset, 2, 2);
                ctx.fillRect(31, 15 + nodOffset, 2, 2);
            }

            // Petites moustaches / bouche sage
            ctx.fillStyle = '#cbd5e1';
            ctx.fillRect(24, 25 + nodOffset, 8, 2);

            // Corps + col blouse psy
            ctx.fillStyle = '#1e293b';
            ctx.fillRect(16, 31, 24, 18);
            ctx.fillStyle = '#e2e8f0';
            ctx.fillRect(22, 31, 12, 12);
            ctx.fillStyle = '#6366f1';
            ctx.fillRect(27, 32, 2, 9);

            // Bloc-notes & stylo animé quand il réfléchit
            const penY = this.psyState === 'thinking' ? 37 + Math.round(Math.sin(tick * 0.5) * 2) : 39;
            ctx.fillStyle = '#fef3c7';
            ctx.fillRect(36, 36, 11, 13);
            ctx.fillStyle = '#94a3b8';
            ctx.fillRect(38, 39, 7, 1);
            ctx.fillRect(38, 42, 6, 1);
            ctx.fillStyle = '#ef4444';
            ctx.fillRect(42, penY, 4, 2);

            requestAnimationFrame(draw);
        };
        requestAnimationFrame(draw);
    }
}
customElements.define('chat-view', ChatView);
