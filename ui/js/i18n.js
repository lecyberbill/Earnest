/**
 * Helper i18n léger pour Earnest Agent Operations Center (FR / EN).
 */
(function () {
    const TRANSLATIONS = {
        fr: {
            'app.title': 'Earnest • Station d\'Opérations Agentiques',
            'hud.subtitle': 'Agent Operations Center',
            'hud.probing_models': 'Sondage des modèles...',
            'hud.runs': 'Runs',
            'hud.memories': 'Mémoires',
            'hud.tools': 'Outils',
            'hud.probe': 'Sonde',
            'hud.probing': 'Sonde...',
            'hud.probe_title': 'Sonder à nouveau les moteurs en ligne',
            'hud.lang_title': 'Langue de l\'interface',

            'nav.run': 'Console de Run',
            'nav.chat': 'Conversation',
            'nav.history': 'Historique',
            'nav.memories': 'Mémoires Persistantes',
            'nav.skills': 'Compétences & Outils',
            'nav.artifacts': 'Artefacts Générés',
            'nav.tasks': 'Tâches Programmées',
            'nav.workflows': 'Workflows (WFL)',
            'wf.title': 'Orchestrateur de Workflows WFL',
            'wf.desc': 'Gestion des flux déterministes (.wf.yaml) : dossiers, activation, scellage SHA-256, cron et audit SQLite WAL.',
            'wf.col_name': 'Workflow',
            'wf.col_folder': 'Dossier',
            'wf.col_seal': 'Statut',
            'wf.col_active': 'Actif',
            'wf.col_cron': 'Cron',
            'wf.col_actions': 'Actions',
            'wf.loading': 'Chargement des workflows...',
            'wf.runs_title': 'Historique des Runs WFL',
            'wf.inspector_title': 'Inspecteur StepLogs (SQLite WAL)',
            'nav.evals': 'Évals & Benchmarks',
            'nav.logs': 'Logs',
            'nav.settings': 'Configuration LLM',
            'sidebar.footer_mode': 'Zéro Démon • Run-to-0',

            // Run Console
            'run.title': 'Console Opérationnelle & Streaming ReAct',
            'run.desc': 'Exécution d\'instructions agentiques avec traçabilité complète et monitoring des modèles en direct.',
            'run.prompt_placeholder': 'Saisissez une instruction ou collez votre consigne ici (ex: Analyse ce rapport de synthèse, identifie les points clés et génère un livrable)...',
            'run.routing_label': 'Routage :',
            'run.route_auto': 'Routage Auto (Intelligent)',
            'run.route_multiagent': 'Mode Multi-Agents (Architecte + Worker + Critique)',
            'run.route_ollama': 'Moteur Local Principal (Ollama)',
            'run.route_lmstudio': 'Moteur Local Secondaire (LM Studio)',
            'run.route_cloud': 'API Cloud (Haute Capacité)',
            'run.effort_title': 'Niveau de réflexion (Thinking / Reasoning)',
            'run.attach_title': 'Joindre un fichier (Image, PDF, Word, Code)',
            'run.mic_title': 'Dictée Vocale du Prompt',
            'run.stop_title': 'Interrompre le run en cours',
            'run.stop': 'Arrêter',
            'run.execute': 'Exécuter',
            'run.attached_label': 'Fichier joint :',
            'run.remove_att': 'Retirer',
            'run.hitl_title': 'VALIDATION REQUISE (HUMAN-IN-THE-LOOP)',
            'run.hitl_desc': 'L\'agent souhaite exécuter une commande sensible ayant un impact potentiel :',

            // Context Inspector
            'inspector.history_title': 'Historique des Exécutions',
            'inspector.history_loading': 'Chargement de l\'historique...',
            'inspector.history_empty': 'Aucun run archivé.',
            'inspector.trace_title': 'Inspecteur de Trace Sélectée',
            'inspector.trace_placeholder': 'Cliquez sur un run ci-dessus pour inspecter son audit complet.',

            // Chat View
            'chat.title': 'Salon de Discussion Directe',
            'chat.status_listening': 'À l\'écoute',
            'chat.desc': 'Discussion continue persistée • Choisis ton modèle et discute avec Dr. Earnest.',
            'chat.model_auto': 'Modèle : Auto (Priorité)',
            'chat.new_session': 'Nouvelle Session',
            'chat.new_thread': 'Nouveau Fil',
            'chat.welcome': 'Bonjour ! Je suis Earnest. Tu peux me poser des questions, me demander de réfléchir à un sujet ou discuter de ton projet librement ici.',
            'chat.input_placeholder': 'Écrivez ou dictez votre message (Entrée pour envoyer)...',

            // Memory Grid
            'mem.title': 'Inspecteur & Éditeur de Mémoires',
            'mem.desc': 'Système de persistance et de rappel contextuel isolé par projet et par session.',
            'mem.key_placeholder': 'Clé (ex: arch_target)',
            'mem.content_placeholder': 'Contenu textuel ou Markdown...',
            'mem.scope_project': 'Project (Permanent)',
            'mem.scope_session': 'Session (Éphémère)',
            'mem.add_btn': 'Ajouter',
            'mem.search_placeholder': 'Recherche sémantique RAG & plein-texte FTS5 dans les mémoires...',
            'mem.search_btn': 'Rechercher',
            'mem.col_scope': 'Scope',
            'mem.col_key': 'Clé',
            'mem.col_preview': 'Aperçu du Contenu',
            'mem.col_details': 'Détails',
            'mem.col_updated': 'Dernière MàJ',
            'mem.loading': 'Chargement des mémoires...',
            'mem.modal_label': 'Contenu intégral stocké :',

            // Skills View
            'skills.title': 'Registre des Compétences & Outils (Skills)',
            'skills.desc': 'Toutes les fonctions disponibles pour l\'agent ReAct : primitives système intégrées, compétences auto-générées et serveurs MCP tiers.',
            'skills.col_name': 'Nom de l\'Outil',
            'skills.col_type': 'Type',
            'skills.col_desc': 'Description',
            'skills.col_entry': 'Point d\'entrée',
            'skills.col_action': 'Action',
            'skills.loading': 'Chargement des compétences...',
            'skills.schema_title': 'Paramètres & Schéma JSON',
            'skills.schema_placeholder': 'Cliquez sur un outil ci-dessus pour inspecter sa signature.',
            'skills.bench_title': 'Banc d\'Essai Unitaire',
            'skills.test_btn': 'Tester l\'Outil',
            'skills.bench_desc': 'Saisissez les arguments au format JSON pour tester directement l\'exécution de l\'outil sélectionné :',
            'skills.result_label': 'Résultat de l\'exécution :',
            'skills.result_waiting': 'En attente de test...',

            // Artifacts View
            'art.title': 'Galerie & Visualiseur d\'Artefacts',
            'art.desc': 'Fichiers générés, rapports Markdown, code source et schémas matérialisés sur le disque.',
            'art.refresh': 'Rafraîchir',
            'art.loading': 'Chargement des artefacts...',
            'art.select_file': 'Sélectionnez un fichier',
            'art.edit': 'Éditer',
            'art.save': 'Enregistrer',
            'art.copy': 'Copier',
            'art.delete': 'Supprimer',
            'art.empty_preview': 'Aucun artefact sélectionné. Cliquez sur un élément dans la liste à gauche.',

            // Task Planner
            'tasks.title': 'Planificateur de Tâches Périodiques',
            'tasks.desc': 'Ordonnancement autonome des missions récurrentes délégué au planificateur système natif de l\'hôte.',
            'tasks.name_placeholder': 'Nom (ex: sync_veille_techno)',
            'tasks.prompt_placeholder': 'Instruction agentique à déclencher...',
            'tasks.add_btn': 'Programmer',
            'tasks.col_name': 'Nom de la Tâche',
            'tasks.col_freq': 'Fréquence',
            'tasks.col_prompt': 'Instruction Déclenchée',
            'tasks.col_last': 'Dernière Exécution',
            'tasks.col_status': 'Statut',
            'tasks.loading': 'Chargement des tâches...',

            // Evals View
            'evals.title': 'Harnais d\'Évaluation Automatisé (Agentic Evals)',
            'evals.desc': 'Banc d\'essai automatisé : taux de réussite, fidélité aux outils, vitesse d\'inférence et dérives comportementales.',
            'evals.run_btn': 'Lancer l\'Évaluation Complète',
            'evals.kpi_success': 'Taux de Succès',
            'evals.kpi_score': 'Score Moyen',
            'evals.kpi_time': 'Durée Totale',
            'evals.kpi_tokens': 'Tokens Consommés',
            'evals.scenarios_title': 'Scénarios de Test Standardisés',
            'evals.scenarios_desc': 'Couverture des capacités fondamentales : Raisonnement, Primitives Mémoires, Terminal OS et Artefacts.',
            'evals.running': 'Évaluation en cours sur le modèle actif... Veuillez patienter.',

            // Logs View
            'logs.title': 'Logs Moteur en Direct',
            'logs.desc': 'Flux temps réel de ReActEngine (pensées, appels d\'outils, cache, anti-stall).',
            'logs.filter_placeholder': 'Filtrer (ex: TOOL, WARN, web_search)...',
            'logs.autoscroll': 'Auto-scroll',
            'logs.clear': 'Vider',
            'logs.waiting': 'En attente des logs d\'exécution...',

            // Settings View
            'settings.hub_title': 'Hub Multi-Modèles & Fournisseurs d\'Inférence',
            'settings.hub_desc': 'Déclarez autant de moteurs locaux (Ollama, LM Studio, vLLM) ou cloud (DeepSeek, OpenAI, Groq, Mistral) que souhaité sous forme de tuiles connectables.',
            'settings.refresh_btn': 'Actualiser l\'état',
            'settings.add_model_btn': 'Ajouter un Modèle',
            'settings.form_model_title': 'Déclarer un nouveau point d\'inférence :',
            'settings.lbl_display_name': 'Nom d\'affichage',
            'settings.lbl_api_type': 'Type d\'API',
            'settings.lbl_endpoint': 'URL Endpoint',
            'settings.lbl_model_id': 'Identifiant Modèle',
            'settings.lbl_api_key': 'Clé API (optionnel, chiffrée DPAPI)',
            'settings.save_btn': 'Enregistrer',
            'settings.cancel_btn': 'Annuler',
            'settings.loading_models': 'Chargement des modèles...',
            'settings.res_title': 'Répertoire & Chemins Système Personnalisés',
            'settings.res_desc': 'Indiquez ici n\'importe quel dossier clé, chemin d\'outils, repo git ou documentation que l\'agent doit connaître sur la machine hôte.',
            'settings.res_key_ph': 'Nom de la ressource (ex: repo_git, docs_path, dataset_dir)...',
            'settings.res_val_ph': 'Chemin absolu ou URI (ex: ., C:\\tools)...',
            'settings.res_add_btn': 'Associer',
            'settings.res_col_key': 'Clé de Ressource',
            'settings.res_col_val': 'Chemin ou Valeur',
            'settings.res_col_action': 'Action',
            'settings.res_loading': 'Chargement des ressources...',
            'settings.mcp_title': 'Serveurs MCP Déclarés (JSON-RPC 2.0 / Stdio)',
            'settings.mcp_desc': 'Ajoutez des serveurs MCP locaux ou distants (ex: npx -y @modelcontextprotocol/server-filesystem, python -m mon_serveur_mcp).',
            'settings.mcp_name_ph': 'Nom (ex: filesystem, git, db)...',
            'settings.mcp_cmd_ph': 'Commande (ex: npx, python, node)...',
            'settings.mcp_args_ph': 'Arguments (séparés par espace)...',
            'settings.mcp_add_btn': 'Ajouter MCP',
            'settings.mcp_col_server': 'Serveur',
            'settings.mcp_col_cmd': 'Commande & Arguments',
            'settings.mcp_col_action': 'Action & Test',
            'settings.mcp_loading': 'Chargement des serveurs MCP...',
            'settings.probing_btn': 'Sondage...',
            'settings.no_models': 'Aucun modèle configuré. Cliquez sur "+ Ajouter un Modèle".',
            'settings.status_active': 'Actif / Connecté',
            'settings.status_offline': 'Déconnecté',
            'settings.model_label': 'Modèle :',
            'settings.edit_btn': 'Éditer',
            'settings.connect_btn': 'Connecter',
            'settings.disconnect_btn': 'Déconnecter',
            'settings.delete_model_title': 'Supprimer ce modèle',
            'settings.edit_model_title': 'Modifier le point d\'inférence',
            'settings.missing_model_fields': 'Veuillez renseigner au moins le nom, l\'URL et le modèle.',
            'settings.confirm_delete_model': 'Supprimer définitivement le fournisseur de modèle',
            'settings.no_resources': 'Aucun chemin ou ressource configuré.',
            'settings.delete_btn': 'Supprimer',
            'settings.no_mcp': 'Aucun serveur MCP configuré.',
            'settings.test_btn': 'Tester',
            'settings.missing_mcp_fields': 'Le nom et la commande du serveur MCP sont requis.',
            'settings.mcp_test_ok': 'Serveur MCP \'{name}\' opérationnel !\nOutils détectés ({count}):\n{tools}',
            'settings.mcp_no_tools': 'Aucun outil',
            'settings.mcp_test_fail': 'Échec MCP :',
            'settings.comm_error': 'Erreur de communication :',
        },
        en: {
            'app.title': 'Earnest • Agent Operations System',
            'hud.subtitle': 'Agent Operations Center',
            'hud.probing_models': 'Probing models...',
            'hud.runs': 'Runs',
            'hud.memories': 'Memories',
            'hud.tools': 'Tools',
            'hud.probe': 'Probe',
            'hud.probing': 'Probing...',
            'hud.probe_title': 'Re-probe online engines',
            'hud.lang_title': 'Interface language',

            'nav.run': 'Run Console',
            'nav.chat': 'Conversation',
            'nav.history': 'Execution History',
            'nav.memories': 'Persistent Memories',
            'nav.skills': 'Skills & Tools',
            'nav.artifacts': 'Generated Artifacts',
            'nav.tasks': 'Scheduled Tasks',
            'nav.workflows': 'Workflows (WFL)',
            'wf.title': 'WFL Workflow Orchestrator',
            'wf.desc': 'Deterministic workflow management (.wf.yaml): folders, enable/disable, SHA-256 sealing, cron & SQLite WAL audit.',
            'wf.col_name': 'Workflow',
            'wf.col_folder': 'Folder',
            'wf.col_seal': 'Status',
            'wf.col_active': 'Active',
            'wf.col_cron': 'Cron',
            'wf.col_actions': 'Actions',
            'wf.loading': 'Loading workflows...',
            'wf.runs_title': 'WFL Run History',
            'wf.inspector_title': 'StepLogs Inspector (SQLite WAL)',
            'nav.evals': 'Evals & Benchmarks',
            'nav.logs': 'Logs',
            'nav.settings': 'LLM Settings',
            'sidebar.footer_mode': 'Zero Daemon • Run-to-0',

            // Run Console
            'run.title': 'Operations Console & ReAct Streaming',
            'run.desc': 'Agentic instruction execution with full traceability and live model monitoring.',
            'run.prompt_placeholder': 'Enter an instruction or paste your prompt here (e.g. Analyze this summary report, identify key findings and generate a deliverable)...',
            'run.routing_label': 'Routing:',
            'run.route_auto': 'Auto Routing (Smart)',
            'run.route_multiagent': 'Multi-Agent Mode (Architect + Worker + Critic)',
            'run.route_ollama': 'Primary Local Engine (Ollama)',
            'run.route_lmstudio': 'Secondary Local Engine (LM Studio)',
            'run.route_cloud': 'Cloud API (High Capacity)',
            'run.effort_title': 'Reasoning / Thinking effort',
            'run.attach_title': 'Attach a file (Image, PDF, Word, Code)',
            'run.mic_title': 'Voice Dictation',
            'run.stop_title': 'Stop current run',
            'run.stop': 'Stop',
            'run.execute': 'Run',
            'run.attached_label': 'Attached file:',
            'run.remove_att': 'Remove',
            'run.hitl_title': 'APPROVAL REQUIRED (HUMAN-IN-THE-LOOP)',
            'run.hitl_desc': 'The agent wants to execute a sensitive command with potential impact:',

            // Context Inspector
            'inspector.history_title': 'Execution History',
            'inspector.history_loading': 'Loading history...',
            'inspector.history_empty': 'No archived runs.',
            'inspector.trace_title': 'Selected Trace Inspector',
            'inspector.trace_placeholder': 'Click a run above to inspect its full audit trail.',

            // Chat View
            'chat.title': 'Direct Discussion Room',
            'chat.status_listening': 'Listening',
            'chat.desc': 'Persistent continuous chat • Pick your model and talk with Dr. Earnest.',
            'chat.model_auto': 'Model: Auto (Priority)',
            'chat.new_session': 'New Session',
            'chat.new_thread': 'New Thread',
            'chat.welcome': 'Hello! I am Earnest. Ask me anything, brainstorm a topic, or discuss your project freely here.',
            'chat.input_placeholder': 'Type or dictate your message (Enter to send)...',

            // Memory Grid
            'mem.title': 'Memory Inspector & Editor',
            'mem.desc': 'Contextual persistence and recall system isolated by project and session.',
            'mem.key_placeholder': 'Key (e.g. arch_target)',
            'mem.content_placeholder': 'Text or Markdown content...',
            'mem.scope_project': 'Project (Permanent)',
            'mem.scope_session': 'Session (Ephemeral)',
            'mem.add_btn': 'Add',
            'mem.search_placeholder': 'Semantic RAG & full-text FTS5 search across memories...',
            'mem.search_btn': 'Search',
            'mem.col_scope': 'Scope',
            'mem.col_key': 'Key',
            'mem.col_preview': 'Content Preview',
            'mem.col_details': 'Details',
            'mem.col_updated': 'Last Updated',
            'mem.loading': 'Loading memories...',
            'mem.modal_label': 'Full stored content:',

            // Skills View
            'skills.title': 'Skills & Tools Registry',
            'skills.desc': 'All functions available to the ReAct agent: built-in system primitives, auto-generated skills and external MCP servers.',
            'skills.col_name': 'Tool Name',
            'skills.col_type': 'Type',
            'skills.col_desc': 'Description',
            'skills.col_entry': 'Entrypoint',
            'skills.col_action': 'Action',
            'skills.loading': 'Loading skills...',
            'skills.schema_title': 'Parameters & JSON Schema',
            'skills.schema_placeholder': 'Click a tool above to inspect its signature.',
            'skills.bench_title': 'Unit Test Bench',
            'skills.test_btn': 'Test Tool',
            'skills.bench_desc': 'Enter JSON arguments to directly test executing the selected tool:',
            'skills.result_label': 'Execution result:',
            'skills.result_waiting': 'Waiting for test...',

            // Artifacts View
            'art.title': 'Artifacts Gallery & Viewer',
            'art.desc': 'Generated files, Markdown reports, source code and schemas saved on disk.',
            'art.refresh': 'Refresh',
            'art.loading': 'Loading artifacts...',
            'art.select_file': 'Select a file',
            'art.edit': 'Edit',
            'art.save': 'Save',
            'art.copy': 'Copy',
            'art.delete': 'Delete',
            'art.empty_preview': 'No artifact selected. Click an item in the list on the left.',

            // Task Planner
            'tasks.title': 'Periodic Task Scheduler',
            'tasks.desc': 'Autonomous scheduling of recurring missions delegated to the host OS native scheduler.',
            'tasks.name_placeholder': 'Name (e.g. sync_tech_watch)',
            'tasks.prompt_placeholder': 'Agentic instruction to trigger...',
            'tasks.add_btn': 'Schedule',
            'tasks.col_name': 'Task Name',
            'tasks.col_freq': 'Frequency',
            'tasks.col_prompt': 'Triggered Instruction',
            'tasks.col_last': 'Last Run',
            'tasks.col_status': 'Status',
            'tasks.loading': 'Loading tasks...',

            // Evals View
            'evals.title': 'Automated Evaluation Harness (Agentic Evals)',
            'evals.desc': 'Automated test bench: success rate, tool fidelity, inference speed and behavioral drift.',
            'evals.run_btn': 'Run Full Evaluation',
            'evals.kpi_success': 'Success Rate',
            'evals.kpi_score': 'Average Score',
            'evals.kpi_time': 'Total Duration',
            'evals.kpi_tokens': 'Tokens Used',
            'evals.scenarios_title': 'Standardized Test Scenarios',
            'evals.scenarios_desc': 'Coverage of core capabilities: Reasoning, Memory Primitives, OS Terminal and Artifacts.',
            'evals.running': 'Evaluation running on active model... Please wait.',

            // Logs View
            'logs.title': 'Live Engine Logs',
            'logs.desc': 'Real-time stream from ReActEngine (thoughts, tool calls, cache, anti-stall).',
            'logs.filter_placeholder': 'Filter (e.g. TOOL, WARN, web_search)...',
            'logs.autoscroll': 'Auto-scroll',
            'logs.clear': 'Clear',
            'logs.waiting': 'Waiting for execution logs...',

            // Settings View
            'settings.hub_title': 'Multi-Model Hub & Inference Providers',
            'settings.hub_desc': 'Register as many local engines (Ollama, LM Studio, vLLM) or cloud APIs (DeepSeek, OpenAI, Groq, Mistral) as needed as connectable tiles.',
            'settings.refresh_btn': 'Refresh Status',
            'settings.add_model_btn': 'Add Model',
            'settings.form_model_title': 'Register a new inference endpoint:',
            'settings.lbl_display_name': 'Display Name',
            'settings.lbl_api_type': 'API Type',
            'settings.lbl_endpoint': 'Endpoint URL',
            'settings.lbl_model_id': 'Model Identifier',
            'settings.lbl_api_key': 'API Key (optional, DPAPI encrypted)',
            'settings.save_btn': 'Save',
            'settings.cancel_btn': 'Cancel',
            'settings.loading_models': 'Loading models...',
            'settings.res_title': 'Custom System Paths & Resources',
            'settings.res_desc': 'Specify key folders, tool paths, git repos or documentation the agent should know about on the host machine.',
            'settings.res_key_ph': 'Resource name (e.g. repo_git, docs_path, dataset_dir)...',
            'settings.res_val_ph': 'Absolute path or URI (e.g. ., C:\\tools)...',
            'settings.res_add_btn': 'Bind',
            'settings.res_col_key': 'Resource Key',
            'settings.res_col_val': 'Path or Value',
            'settings.res_col_action': 'Action',
            'settings.res_loading': 'Loading resources...',
            'settings.mcp_title': 'Registered MCP Servers (JSON-RPC 2.0 / Stdio)',
            'settings.mcp_desc': 'Add local or remote MCP servers (e.g. npx -y @modelcontextprotocol/server-filesystem, python -m my_mcp_server).',
            'settings.mcp_name_ph': 'Name (e.g. filesystem, git, db)...',
            'settings.mcp_cmd_ph': 'Command (e.g. npx, python, node)...',
            'settings.mcp_args_ph': 'Arguments (space-separated)...',
            'settings.mcp_add_btn': 'Add MCP',
            'settings.mcp_col_server': 'Server',
            'settings.mcp_col_cmd': 'Command & Arguments',
            'settings.mcp_col_action': 'Action & Test',
            'settings.mcp_loading': 'Loading MCP servers...',
            'settings.probing_btn': 'Probing...',
            'settings.no_models': 'No model configured. Click "+ Add Model".',
            'settings.status_active': 'Active / Connected',
            'settings.status_offline': 'Disconnected',
            'settings.model_label': 'Model:',
            'settings.edit_btn': 'Edit',
            'settings.connect_btn': 'Connect',
            'settings.disconnect_btn': 'Disconnect',
            'settings.delete_model_title': 'Delete this model',
            'settings.edit_model_title': 'Edit inference endpoint',
            'settings.missing_model_fields': 'Please fill in at least the name, URL, and model.',
            'settings.confirm_delete_model': 'Permanently delete model provider',
            'settings.no_resources': 'No custom path or resource configured.',
            'settings.delete_btn': 'Delete',
            'settings.no_mcp': 'No MCP server configured.',
            'settings.test_btn': 'Test',
            'settings.missing_mcp_fields': 'MCP server name and command are required.',
            'settings.mcp_test_ok': 'MCP server \'{name}\' operational!\nDetected tools ({count}):\n{tools}',
            'settings.mcp_no_tools': 'No tools',
            'settings.mcp_test_fail': 'MCP Error:',
            'settings.comm_error': 'Communication error:',
        }
    };

    let currentLang = localStorage.getItem('earnest_ops_lang') || 'fr';
    if (!TRANSLATIONS[currentLang]) currentLang = 'fr';

    window.t = function (key) {
        return (TRANSLATIONS[currentLang] && TRANSLATIONS[currentLang][key])
            || TRANSLATIONS.fr[key]
            || key;
    };

    window.getLang = function () {
        return currentLang;
    };

    window.applyI18n = function (root = document) {
        document.documentElement.lang = currentLang;
        document.title = window.t('app.title');
        root.querySelectorAll('[data-i18n]').forEach((el) => {
            const key = el.getAttribute('data-i18n');
            if (key) el.textContent = window.t(key);
        });
        root.querySelectorAll('[data-i18n-placeholder]').forEach((el) => {
            const key = el.getAttribute('data-i18n-placeholder');
            if (key) el.setAttribute('placeholder', window.t(key));
        });
        root.querySelectorAll('[data-i18n-title]').forEach((el) => {
            const key = el.getAttribute('data-i18n-title');
            if (key) el.setAttribute('title', window.t(key));
        });
    };

    window.setLang = function (lang) {
        if (!TRANSLATIONS[lang]) return;
        currentLang = lang;
        localStorage.setItem('earnest_ops_lang', lang);
        window.applyI18n();
        const sel = document.getElementById('hud-lang-select');
        if (sel) sel.value = lang;
        window.dispatchEvent(new CustomEvent('earnest:lang_changed', { detail: { lang } }));
    };

    document.addEventListener('DOMContentLoaded', () => {
        window.applyI18n();
        const sel = document.getElementById('hud-lang-select');
        if (sel) {
            sel.value = currentLang;
            sel.addEventListener('change', (e) => window.setLang(e.target.value));
        }
    });
})();
