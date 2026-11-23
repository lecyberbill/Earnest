/**
 * Rend cliquables les références de fichiers (`src/lib.rs`, `src/lib.rs:42`) dans le flux de chat.
 */
export function initChatFileLinks({ chatStream, openFileAt }) {
    if (!chatStream || !openFileAt) return;
    const filePattern = /^([A-Za-z0-9_.\-\/\\]+\.(?:rs|py|js|ts|tsx|jsx|html|css|json|toml|yaml|yml|md|sh|sql))(?::(\d+)|#L(\d+))?$/i;

    chatStream.addEventListener('click', (e) => {
        const target = e.target.closest('code, a');
        if (!target || !chatStream.contains(target)) return;
        const raw = (target.textContent || '').trim();
        const m = raw.match(filePattern);
        if (!m) return;
        e.preventDefault();
        const filePath = m[1];
        const line = parseInt(m[2] || m[3] || '1', 10);
        openFileAt(filePath, Number.isFinite(line) ? line : 1);
    });
}
