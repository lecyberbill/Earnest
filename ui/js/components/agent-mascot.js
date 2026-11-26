// Web Component <agent-mascot> — Robot SVG animé piloté par l'état du run (sleep | think | work)
class AgentMascot extends HTMLElement {
    constructor() {
        super();
        this.attachShadow({ mode: 'open' });
        this._state = 'sleep';
    }

    connectedCallback() {
        this.render();
        this.setState(this._state);
    }

    setState(state) {
        this._state = state;
        const root = this.shadowRoot;
        if (!root) return;

        const wrap = root.getElementById('mascot-wrap');
        const master = root.getElementById('robot-master');
        const head = root.getElementById('robot-head');
        const antenna = root.getElementById('robot-antenna');
        const bulb = root.getElementById('antenna-bulb');
        const chest = root.getElementById('chest-light');
        const earL = root.getElementById('ear-left');
        const earR = root.getElementById('ear-right');
        const armL = root.getElementById('arm-left');
        const armR = root.getElementById('arm-right');
        const fWork = root.getElementById('face-work');
        const fThink = root.getElementById('face-think');
        const fSleep = root.getElementById('face-sleep');
        const pWork = root.getElementById('prop-work');
        const pThink = root.getElementById('prop-think');
        const pSleep = root.getElementById('prop-sleep');
        if (!wrap || !master) return;

        [fWork, fThink, fSleep, pWork, pThink, pSleep].forEach(el => {
            if (el) el.style.opacity = '0';
        });
        master.className.baseVal = '';
        head.className.baseVal = '';
        antenna.className.baseVal = '';
        bulb.className.baseVal = '';
        armL.className.baseVal = '';
        armR.className.baseVal = '';
        armR.style.transform = '';
        wrap.className = 'wrap ' + state;

        if (state === 'work') {
            [bulb, chest, earL, earR].forEach(e => e && e.setAttribute('fill', '#38bdf8'));
            master.classList.add('robot-body-work');
            bulb.classList.add('antenna-tip-work');
            armL.classList.add('arm-left-work');
            armR.classList.add('arm-right-work');
            fWork.style.opacity = '1';
            pWork.style.opacity = '1';
            wrap.title = 'Agent au travail (outils / exécution)';
        } else if (state === 'think') {
            [bulb, chest, earL, earR].forEach(e => e && e.setAttribute('fill', '#c084fc'));
            master.classList.add('robot-body-think');
            head.classList.add('head-think');
            antenna.classList.add('antenna-group-think');
            armR.style.transform = 'translate(-22px, -30px) rotate(35deg)';
            armR.style.transformOrigin = '214px 195px';
            fThink.style.opacity = '1';
            pThink.style.opacity = '1';
            wrap.title = 'Agent en réflexion (LLM)';
        } else {
            [bulb, chest, earL, earR].forEach(e => e && e.setAttribute('fill', '#818cf8'));
            master.classList.add('robot-body-sleep');
            head.classList.add('head-sleep');
            fSleep.style.opacity = '1';
            pSleep.style.opacity = '1';
            wrap.title = 'Agent en veille';
        }
    }

    render() {
        const isPsy = this.getAttribute('variant') === 'psy';
        const glassesHtml = isPsy
            ? `<g id="psy-glasses" stroke="#fbbf24" stroke-width="3.2" fill="rgba(56,189,248,0.12)">
                   <circle cx="126" cy="115" r="15"/>
                   <circle cx="174" cy="115" r="15"/>
                   <line x1="141" y1="115" x2="159" y2="115" stroke-linecap="round"/>
               </g>`
            : '';
        this.shadowRoot.innerHTML = `
            <style>
                :host { display: inline-block; width: 78px; height: 78px; flex-shrink: 0; }
                .wrap {
                    width: 78px; height: 78px; border-radius: 16px;
                    border: 1px solid rgba(148, 163, 184, 0.22);
                    display: flex; align-items: center; justify-content: center;
                    transition: background 0.5s ease, border-color 0.5s ease;
                    overflow: hidden;
                }
                .wrap.work { background: radial-gradient(circle at 50% 35%, rgba(56,189,248,0.22), rgba(15,23,42,0.85)); border-color: rgba(56,189,248,0.45); }
                .wrap.think { background: radial-gradient(circle at 50% 35%, rgba(192,132,252,0.22), rgba(15,23,42,0.85)); border-color: rgba(192,132,252,0.45); }
                .wrap.sleep { background: radial-gradient(circle at 50% 35%, rgba(99,102,241,0.18), rgba(15,23,42,0.85)); border-color: rgba(129,140,248,0.3); }
                svg { width: 100%; height: 100%; }
                @keyframes float { 0%,100% { transform: translateY(0); } 50% { transform: translateY(-5px); } }
                @keyframes sleepBreath { 0%,100% { transform: scaleY(1) translateY(0); } 50% { transform: scaleY(0.96) translateY(3px); } }
                @keyframes headTilt { 0%,100% { transform: rotate(0); } 35%,65% { transform: rotate(-7deg); } }
                @keyframes typingL { 0%,100% { transform: translateY(0) rotate(0); } 50% { transform: translateY(-7px) rotate(-10deg); } }
                @keyframes typingR { 0%,100% { transform: translateY(-7px) rotate(10deg); } 50% { transform: translateY(0) rotate(0); } }
                @keyframes antPulse { 0%,100% { opacity: 1; } 50% { opacity: 0.35; } }
                @keyframes antWiggle { 0%,100% { transform: rotate(0); } 25% { transform: rotate(-12deg); } 75% { transform: rotate(12deg); } }
                @keyframes gearSpin { from { transform: rotate(0); } to { transform: rotate(360deg); } }
                @keyframes zzz { 0% { transform: translate(0,0) scale(0.6); opacity: 0; } 40% { opacity: 1; } 100% { transform: translate(16px,-26px) scale(1.1); opacity: 0; } }
                .robot-body-work { animation: float 2.4s ease-in-out infinite; }
                .robot-body-think { animation: float 3.2s ease-in-out infinite; }
                .robot-body-sleep { animation: sleepBreath 4s ease-in-out infinite; transform-origin: 150px 250px; }
                .head-think { animation: headTilt 4s ease-in-out infinite; transform-origin: 150px 160px; }
                .head-sleep { transform: rotate(7deg) translateY(5px); transform-origin: 150px 160px; transition: transform 0.5s ease; }
                .arm-left-work { animation: typingL 0.35s ease-in-out infinite; transform-origin: 85px 185px; }
                .arm-right-work { animation: typingR 0.35s ease-in-out infinite; transform-origin: 215px 185px; }
                .antenna-tip-work { animation: antPulse 0.8s infinite; }
                .antenna-group-think { animation: antWiggle 2s ease-in-out infinite; transform-origin: 150px 75px; }
                .gear-1 { animation: gearSpin 4s linear infinite; transform-origin: 218px 76px; }
                .zzz-1 { animation: zzz 2.8s ease-out infinite; }
            </style>
            <div id="mascot-wrap" class="wrap sleep">
                <svg viewBox="0 0 300 300" xmlns="http://www.w3.org/2000/svg">
                    <defs>
                        <linearGradient id="bGrad" x1="0%" y1="0%" x2="100%" y2="100%"><stop offset="0%" stop-color="#f8fafc"/><stop offset="100%" stop-color="#cbd5e1"/></linearGradient>
                        <linearGradient id="sGrad" x1="0%" y1="0%" x2="0%" y2="100%"><stop offset="0%" stop-color="#1e293b"/><stop offset="100%" stop-color="#0f172a"/></linearGradient>
                    </defs>
                    <g id="robot-master">
                        <rect x="132" y="152" width="36" height="16" rx="6" fill="#94a3b8"/>
                        <rect x="102" y="164" width="96" height="72" rx="28" fill="url(#bGrad)" stroke="#94a3b8" stroke-width="3"/>
                        <circle id="chest-light" cx="150" cy="194" r="7" fill="#818cf8"/>
                        <g id="robot-head">
                            <g id="robot-antenna">
                                <line x1="150" y1="76" x2="150" y2="48" stroke="#94a3b8" stroke-width="5" stroke-linecap="round"/>
                                <circle id="antenna-bulb" cx="150" cy="42" r="8" fill="#818cf8"/>
                            </g>
                            <rect x="74" y="102" width="14" height="34" rx="7" fill="#64748b"/>
                            <rect id="ear-left" x="70" y="108" width="6" height="22" rx="3" fill="#818cf8"/>
                            <rect x="212" y="102" width="14" height="34" rx="7" fill="#64748b"/>
                            <rect id="ear-right" x="224" y="108" width="6" height="22" rx="3" fill="#818cf8"/>
                            <rect x="84" y="74" width="132" height="86" rx="38" fill="url(#bGrad)" stroke="#94a3b8" stroke-width="3"/>
                            <rect x="96" y="86" width="108" height="62" rx="26" fill="url(#sGrad)"/>
                            <g id="face-work" style="opacity:0">
                                <circle cx="126" cy="115" r="10" fill="#38bdf8"/><circle cx="123" cy="112" r="3" fill="#fff"/>
                                <circle cx="174" cy="115" r="10" fill="#38bdf8"/><circle cx="171" cy="112" r="3" fill="#fff"/>
                                <path d="M143 126 Q150 132 157 126" stroke="#38bdf8" stroke-width="3" stroke-linecap="round" fill="none"/>
                            </g>
                            <g id="face-think" style="opacity:0">
                                <circle cx="126" cy="112" r="10" fill="#c084fc"/><circle cx="129" cy="108" r="3" fill="#fff"/>
                                <path d="M164 115 Q174 108 184 115" stroke="#c084fc" stroke-width="4" stroke-linecap="round" fill="none"/>
                                <path d="M144 128 Q150 124 156 128" stroke="#c084fc" stroke-width="3" stroke-linecap="round" fill="none"/>
                            </g>
                            <g id="face-sleep" style="opacity:1">
                                <path d="M116 116 Q126 124 136 116" stroke="#818cf8" stroke-width="3.5" stroke-linecap="round" fill="none"/>
                                <path d="M164 116 Q174 124 184 116" stroke="#818cf8" stroke-width="3.5" stroke-linecap="round" fill="none"/>
                                <circle cx="150" cy="128" r="3.5" fill="#818cf8" opacity="0.8"/>
                            </g>
                            ${glassesHtml}
                        </g>
                        <g id="arm-left"><rect x="74" y="176" width="24" height="42" rx="12" fill="url(#bGrad)" stroke="#94a3b8" stroke-width="3"/></g>
                        <g id="arm-right"><rect x="202" y="176" width="24" height="42" rx="12" fill="url(#bGrad)" stroke="#94a3b8" stroke-width="3"/></g>
                    </g>
                    <g id="prop-work" style="opacity:0">
                        <path d="M92 246 L208 246 L216 254 L84 254 Z" fill="#64748b"/>
                        <rect x="96" y="206" width="108" height="42" rx="8" fill="#334155" stroke="#475569" stroke-width="2"/>
                        <circle cx="150" cy="227" r="7" fill="#38bdf8"/>
                    </g>
                    <g id="prop-think" style="opacity:0">
                        <circle cx="195" cy="115" r="4" fill="#1e293b" stroke="#c084fc" stroke-width="2"/>
                        <rect x="196" y="54" width="60" height="44" rx="18" fill="#1e293b" stroke="#c084fc" stroke-width="2.5"/>
                        <g class="gear-1"><circle cx="218" cy="76" r="8" fill="none" stroke="#c084fc" stroke-width="3" stroke-dasharray="4 2"/></g>
                    </g>
                    <g id="prop-sleep" style="opacity:1">
                        <text x="198" y="102" font-family="sans-serif" font-weight="700" font-size="22" fill="#a5b4fc" class="zzz-1">Z</text>
                    </g>
                </svg>
            </div>
        `;
    }
}
customElements.define('agent-mascot', AgentMascot);
