// Greffon Whisper.cpp à la demande (vérification, installation partagée, encodage WAV 16kHz)
window.ensureWhisperReady = async function () {
    try {
        const res = await fetch('/api/transcribe/status');
        const st = await res.json();
        if (st.installed) return true;
    } catch (e) {
        alert('Whisper status error: ' + e.message);
        return false;
    }

    const lang = (window.getLang ? window.getLang() : (localStorage.getItem('earnest_ops_lang') || 'fr')).startsWith('en') ? 'en' : 'fr';
    const msg = lang === 'en'
        ? 'Voice dictation requires downloading the local Whisper.cpp addon (~575 MB, large-v3-turbo-q5_0) into your Earnest data folder. Download and install it now?'
        : 'La dictée vocale nécessite le téléchargement du greffon local Whisper.cpp (~575 Mo, large-v3-turbo-q5_0) dans votre dossier de données Earnest. Télécharger et installer maintenant ?';
    if (!confirm(msg)) return false;

    const banner = document.createElement('div');
    banner.id = 'whisper-install-banner';
    banner.style.cssText = 'position:fixed;bottom:20px;right:20px;z-index:9999;background:#0f172a;border:1px solid #38bdf8;color:#e2e8f0;padding:12px 18px;border-radius:8px;font-family:sans-serif;font-size:0.9rem;box-shadow:0 8px 24px rgba(0,0,0,0.5);';
    banner.textContent = lang === 'en' ? '⏳ Installing Whisper.cpp, please wait...' : '⏳ Installation de Whisper.cpp en cours, veuillez patienter...';
    document.body.appendChild(banner);

    try {
        const installRes = await fetch('/api/transcribe/install');
        const installData = await installRes.json();
        if (installData.status === 'error') {
            banner.remove();
            alert((lang === 'en' ? 'Whisper install failed: ' : 'Échec installation Whisper : ') + (installData.error || 'unknown'));
            return false;
        }
        // Polling /api/transcribe/status jusqu'à la fin du téléchargement (max 20 min)
        for (let i = 0; i < 600; i++) {
            await new Promise(r => setTimeout(r, 2000));
            const stRes = await fetch('/api/transcribe/status');
            const st = await stRes.json();
            const mb = st.downloaded_mb || 0;
            banner.textContent = lang === 'en'
                ? `⏳ Installing Whisper.cpp (${mb} MB / ~580 MB)...`
                : `⏳ Installation de Whisper.cpp (${mb} Mo / ~580 Mo)...`;
            if (st.installed) {
                banner.remove();
                alert(lang === 'en' ? '✅ Whisper is ready! Click the mic to start.' : '✅ Whisper est prêt ! Cliquez sur le micro pour parler.');
                return true;
            }
            if (st.error) {
                banner.remove();
                alert((lang === 'en' ? 'Whisper install failed: ' : 'Échec installation Whisper : ') + st.error);
                return false;
            }
        }
        banner.remove();
        alert(lang === 'en' ? 'Whisper install timed out.' : 'Délai dépassé pour l\'installation de Whisper.');
        return false;
    } catch (e) {
        banner.remove();
        alert((lang === 'en' ? 'Whisper install error: ' : 'Erreur installation Whisper : ') + e.message);
        return false;
    }
};

window.blobToWavBase64 = async function (blob) {
    const arrayBuffer = await blob.arrayBuffer();
    const audioCtx = new (window.AudioContext || window.webkitAudioContext)({ sampleRate: 16000 });
    const decoded = await audioCtx.decodeAudioData(arrayBuffer);
    const channelData = decoded.getChannelData(0);
    const pcm16 = new Int16Array(channelData.length);
    for (let i = 0; i < channelData.length; i++) {
        const s = Math.max(-1, Math.min(1, channelData[i]));
        pcm16[i] = s < 0 ? s * 0x8000 : s * 0x7FFF;
    }
    const wavBuffer = new ArrayBuffer(44 + pcm16.length * 2);
    const view = new DataView(wavBuffer);
    const writeStr = (off, str) => { for (let i = 0; i < str.length; i++) view.setUint8(off + i, str.charCodeAt(i)); };
    writeStr(0, 'RIFF');
    view.setUint32(4, 36 + pcm16.length * 2, true);
    writeStr(8, 'WAVE');
    writeStr(12, 'fmt ');
    view.setUint32(16, 16, true);
    view.setUint16(20, 1, true);
    view.setUint16(22, 1, true);
    view.setUint32(24, 16000, true);
    view.setUint32(28, 32000, true);
    view.setUint16(32, 2, true);
    view.setUint16(34, 16, true);
    writeStr(36, 'data');
    view.setUint32(40, pcm16.length * 2, true);
    new Int16Array(wavBuffer, 44).set(pcm16);

    const bytes = new Uint8Array(wavBuffer);
    let bin = '';
    for (let i = 0; i < bytes.length; i++) bin += String.fromCharCode(bytes[i]);
    return 'data:audio/wav;base64,' + btoa(bin);
};
