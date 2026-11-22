/**
 * Surveillance périodique des fichiers ouverts pour recharger les modifications externes.
 */
export function startFileWatcher({ state, updateDiffModels, intervalMs = 4000 }) {
    let isCheckingExternalModifs = false;

    async function checkExternalModifications() {
        if (isCheckingExternalModifs || state.openTabs.length === 0) return;
        isCheckingExternalModifs = true;
        try {
            for (const tab of state.openTabs) {
                if (!tab.path || tab.isModified) continue;
                const res = await fetch(`/api/ide/file/status?path=${encodeURIComponent(tab.path)}`);
                const data = await res.json();
                if (data.exists && data.mtime && tab.lastMtime && data.mtime > tab.lastMtime) {
                    const fileRes = await fetch(`/api/ide/file?path=${encodeURIComponent(tab.path)}`);
                    const fileData = await fileRes.json();
                    if (fileData.content !== undefined && fileData.content !== tab.content) {
                        tab.content = fileData.content;
                        tab.originalContent = fileData.content;
                        tab.lastMtime = data.mtime;
                        tab.isModified = false;
                        if (tab.monacoModel) {
                            tab._suppressEvents = true;
                            tab.monacoModel.setValue(fileData.content);
                            tab._suppressEvents = false;
                        }
                        if (state.isDiffMode) updateDiffModels();
                    }
                }
            }
        } catch (e) { } finally {
            isCheckingExternalModifs = false;
        }
    }

    return setInterval(checkExternalModifications, intervalMs);
}
