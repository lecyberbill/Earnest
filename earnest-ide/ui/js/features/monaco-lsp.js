/**
 * Demande les diagnostics LSP en direct et applique les marqueurs Monaco.
 */
export async function refreshLspDiagnostics(editor, tab, rootPath) {
    if (!editor || !tab || !window.monaco) return;
    const model = tab.monacoModel || editor.getModel();
    if (!model) return;
    const lang = model.getLanguageId ? model.getLanguageId() : 'plaintext';
    if (!lang || lang === 'plaintext') return;
    try {
        const res = await fetch('/api/ide/lsp/diagnostics', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({
                language: lang,
                root: rootPath || '.',
                path: tab.path,
                code: model.getValue()
            })
        });
        if (!res.ok) return;
        const data = await res.json();
        const diags = Array.isArray(data.diagnostics) ? data.diagnostics : [];
        const markers = diags.map(d => ({
            startLineNumber: d.start_line || 1,
            startColumn: d.start_col || 1,
            endLineNumber: d.end_line || d.start_line || 1,
            endColumn: d.end_col || (d.start_col ? d.start_col + 1 : 2),
            message: d.message || '',
            severity: d.severity === 1
                ? monaco.MarkerSeverity.Error
                : d.severity === 2
                    ? monaco.MarkerSeverity.Warning
                    : monaco.MarkerSeverity.Info
        }));
        monaco.editor.setModelMarkers(model, 'earnest-lsp', markers);
    } catch (e) { }
}

/**
 * Enregistre les providers LSP Monaco (complétion, survol, définition, signatures)
 * reliés au backend générique JSON-RPC 2.0 /api/ide/lsp/*.
 */
export function registerLspProviders({ state, getActiveTab, openFile, getEditor }) {
    let diagTimer = null;
    const editor = getEditor ? getEditor() : null;
    if (editor && editor.onDidChangeModelContent) {
        editor.onDidChangeModelContent(() => {
            clearTimeout(diagTimer);
            diagTimer = setTimeout(() => {
                const tab = getActiveTab();
                if (tab) refreshLspDiagnostics(getEditor(), tab, state.currentRoot);
            }, 700);
        });
    }
    const supportedLangs = [
        'python', 'rust', 'go', 'c', 'cpp',
        'html', 'css', 'json', 'yaml', 'ini',
        'shell', 'sql', 'markdown'
    ];

    const mapCompletionKind = (kind) => {
        if (kind === 'function' || kind === 'statement') return monaco.languages.CompletionItemKind.Function;
        if (kind === 'class' || kind === 'type') return monaco.languages.CompletionItemKind.Class;
        if (kind === 'module') return monaco.languages.CompletionItemKind.Module;
        if (kind === 'keyword') return monaco.languages.CompletionItemKind.Keyword;
        if (kind === 'variable' || kind === 'param') return monaco.languages.CompletionItemKind.Variable;
        return monaco.languages.CompletionItemKind.Property;
    };

    supportedLangs.forEach(lang => {
        monaco.languages.registerCompletionItemProvider(lang, {
            triggerCharacters: ['.', ':', '>', '/', '_'],
            provideCompletionItems: async function (model, position) {
                const tab = getActiveTab();
                const filePath = tab ? tab.path : `file.${lang}`;
                try {
                    const res = await fetch('/api/ide/lsp/complete', {
                        method: 'POST',
                        headers: { 'Content-Type': 'application/json' },
                        body: JSON.stringify({
                            language: lang,
                            path: filePath,
                            code: model.getValue(),
                            line: position.lineNumber,
                            column: position.column
                        })
                    });
                    const data = await res.json();
                    const word = model.getWordUntilPosition(position);
                    const range = {
                        startLineNumber: position.lineNumber,
                        endLineNumber: position.lineNumber,
                        startColumn: word.startColumn,
                        endColumn: word.endColumn
                    };
                    const suggestions = (data.items || []).map(item => ({
                        label: item.label,
                        kind: mapCompletionKind(item.kind),
                        detail: item.detail || '',
                        documentation: item.doc ? { value: item.doc } : undefined,
                        insertText: item.label,
                        range
                    }));
                    return { suggestions };
                } catch (e) {
                    return { suggestions: [] };
                }
            }
        });

        monaco.languages.registerHoverProvider(lang, {
            provideHover: async function (model, position) {
                const tab = getActiveTab();
                const filePath = tab ? tab.path : `file.${lang}`;
                try {
                    const res = await fetch('/api/ide/lsp/hover', {
                        method: 'POST',
                        headers: { 'Content-Type': 'application/json' },
                        body: JSON.stringify({
                            language: lang,
                            path: filePath,
                            code: model.getValue(),
                            line: position.lineNumber,
                            column: position.column
                        })
                    });
                    const data = await res.json();
                    if (data.contents && data.contents.length > 0) {
                        return {
                            range: new monaco.Range(position.lineNumber, position.column, position.lineNumber, position.column),
                            contents: data.contents.map(c => ({ value: c }))
                        };
                    }
                } catch (e) { }
                return null;
            }
        });

        monaco.languages.registerDefinitionProvider(lang, {
            provideDefinition: async function (model, position) {
                const tab = getActiveTab();
                const filePath = tab ? tab.path : `file.${lang}`;
                const word = model.getWordAtPosition(position);
                const symbol = word ? word.word : '';
                try {
                    const res = await fetch('/api/ide/lsp/definition', {
                        method: 'POST',
                        headers: { 'Content-Type': 'application/json' },
                        body: JSON.stringify({
                            language: lang,
                            root: state.currentRoot,
                            path: filePath,
                            symbol,
                            code: model.getValue(),
                            line: position.lineNumber,
                            column: position.column
                        })
                    });
                    const data = await res.json();
                    if (data.definitions && data.definitions.length > 0) {
                        const def = data.definitions[0];
                        const targetFile = def.file;
                        const targetLine = def.line || 1;
                        const targetCol = def.column || 1;
                        if (targetFile && targetFile !== filePath && !targetFile.endsWith('unknown')) {
                            await openFile(targetFile);
                        }
                        const editor = getEditor();
                        return {
                            uri: (editor && editor.getModel()) ? editor.getModel().uri : model.uri,
                            range: new monaco.Range(targetLine, targetCol, targetLine, targetCol + symbol.length)
                        };
                    }
                } catch (e) { }
                return null;
            }
        });

        monaco.languages.registerSignatureHelpProvider(lang, {
            signatureHelpTriggerCharacters: ['(', ','],
            provideSignatureHelp: async function (model, position) {
                const tab = getActiveTab();
                const filePath = tab ? tab.path : `file.${lang}`;
                try {
                    const res = await fetch('/api/ide/lsp/signatures', {
                        method: 'POST',
                        headers: { 'Content-Type': 'application/json' },
                        body: JSON.stringify({
                            language: lang,
                            path: filePath,
                            code: model.getValue(),
                            line: position.lineNumber,
                            column: position.column
                        })
                    });
                    const data = await res.json();
                    if (data.signatures && data.signatures.length > 0) {
                        return {
                            value: {
                                activeSignature: 0,
                                activeParameter: data.signatures[0].activeParameter || 0,
                                signatures: data.signatures.map(s => ({
                                    label: s.label,
                                    parameters: (s.parameters || []).map(p => ({ label: p }))
                                }))
                            },
                            dispose: () => { }
                        };
                    }
                } catch (e) { }
                return null;
            }
        });
    });
}
