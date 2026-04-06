<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { revealItemInDir } from "@tauri-apps/plugin-opener";
  import { onMount } from "svelte";

  type TemplateVariable = {
    id: number;
    key: string;
    value: string;
  };

  type Tone = "neutral" | "success" | "warning";

  type ApiKeys = {
    clickup_api_key: string;
    gemini_api_key: string;
  };

  type ClickUpList = {
    id: string;
    name: string;
    source: string;
  };

  type GeneratedTask = {
    title: string;
    description: string;
  };

  type VariablePayload = {
    key: string;
    value: string;
  };

  const variableRegex = /(\{\{\s*[A-Z0-9_]+\s*\}\}|\$[A-Z0-9_]+)/g;

  let titleTemplate = $state("{{CLIENTE}} - {{SOLICITUD}}");

  let template = $state(`Crear tarea para {{CLIENTE}}

Resumen:
- Solicitud: {{SOLICITUD}}
- Prioridad: {{PRIORIDAD}}
- Fecha limite: $FECHA_LIMITE

Notas:
{{DETALLE_ADICIONAL}}`);

  let variables = $state<TemplateVariable[]>([
    { id: 1, key: "CLIENTE", value: "Alpha Corp" },
    { id: 2, key: "SOLICITUD", value: "Migracion del flujo de tickets" },
    { id: 3, key: "PRIORIDAD", value: "Alta" },
    { id: 4, key: "FECHA_LIMITE", value: "2026-04-12" },
    { id: 5, key: "DETALLE_ADICIONAL", value: "Adjuntar capturas y validar con operaciones." },
  ]);

  let availableLists = $state<ClickUpList[]>([]);
  let selectedListId = $state("");
  let attachments = $state<File[]>([]);
  let apiKeysPath = $state("Preparando archivo...");
  let statusMessage = $state("Configura tus API keys, genera el borrador y envia la tarea a ClickUp.");
  let statusTone = $state<Tone>("neutral");
  let apiKeys = $state<ApiKeys>({ clickup_api_key: "", gemini_api_key: "" });
  let generatedTask = $state<GeneratedTask | null>(null);
  let createdTaskUrl = $state("");
  let isPreparingApiKeys = $state(false);
  let isLoadingLists = $state(false);
  let isGenerating = $state(false);
  let isSending = $state(false);

  function escapeHtml(value: string) {
    return value
      .replaceAll("&", "&amp;")
      .replaceAll("<", "&lt;")
      .replaceAll(">", "&gt;")
      .replaceAll('"', "&quot;")
      .replaceAll("'", "&#39;");
  }

  let normalizedVariables = $derived.by<VariablePayload[]>(() => {
    return variables
      .map((variable) => ({
        key: sanitizeVariableKey(variable.key),
        value: variable.value.trim(),
      }))
      .filter((variable) => variable.key);
  });

  let highlightedTemplate = $derived.by(() => {
    return escapeHtml(template).replace(variableRegex, '<span class="token">$1</span>');
  });

  let highlightedTitleTemplate = $derived.by(() => {
    return escapeHtml(titleTemplate).replace(variableRegex, '<span class="token">$1</span>');
  });

  let detectedVariables = $derived.by(() => {
    const matches = `${titleTemplate}\n${template}`.match(variableRegex) ?? [];
    return [...new Set(matches.map((token) => token.replace(/[{}\s$]/g, "")))];
  });

  let filledTitlePreview = $derived.by(() => {
    let preview = titleTemplate;

    for (const variable of normalizedVariables) {
      const blockToken = new RegExp(`\\{\\{\\s*${variable.key}\\s*\\}\\}`, "g");
      const inlineToken = new RegExp(`\\$${variable.key}(?![A-Z0-9_])`, "g");

      preview = preview.replace(blockToken, variable.value || `{{${variable.key}}}`);
      preview = preview.replace(inlineToken, variable.value || `$${variable.key}`);
    }

    return preview;
  });

  let filledPreview = $derived.by(() => {
    let preview = template;

    for (const variable of normalizedVariables) {
      const blockToken = new RegExp(`\\{\\{\\s*${variable.key}\\s*\\}\\}`, "g");
      const inlineToken = new RegExp(`\\$${variable.key}(?![A-Z0-9_])`, "g");

      preview = preview.replace(blockToken, variable.value || `{{${variable.key}}}`);
      preview = preview.replace(inlineToken, variable.value || `$${variable.key}`);
    }

    return preview;
  });

  let previewTitle = $derived.by(() => filledTitlePreview || generatedTask?.title || deriveTaskName(filledPreview));
  let previewDescription = $derived.by(() => generatedTask?.description || filledPreview);

  let missingVariables = $derived.by(() => {
    return detectedVariables.filter((name) => {
      const match = normalizedVariables.find((variable) => variable.key === name);
      return !match || !match.value.trim();
    });
  });

  let selectedList = $derived.by(() => {
    return availableLists.find((list) => list.id === selectedListId) ?? null;
  });

  let canLoadClickUpLists = $derived(apiKeys.clickup_api_key.trim().length > 0);

  onMount(async () => {
    await initializeApp();
  });

  function sanitizeVariableKey(value: string) {
    return value.trim().toUpperCase().replace(/[^A-Z0-9_]/g, "_");
  }

  function deriveTaskName(text: string) {
    return (
      text
        .split(/\r?\n/)
        .map((line) => line.trim())
        .find(Boolean)
        ?.slice(0, 120) || "Nueva tarea desde Clikity"
    );
  }

  function setStatus(message: string, tone: Tone = "neutral") {
    statusMessage = message;
    statusTone = tone;
  }

  async function initializeApp() {
    await ensureApiKeysFile(false);
    await refreshApiState();
  }

  async function refreshApiState() {
    try {
      apiKeys = await invoke<ApiKeys>("load_api_keys");

      if (apiKeys.clickup_api_key.trim()) {
        await refreshClickUpLists();
      } else {
        availableLists = [];
        selectedListId = "";
        setStatus("Agrega tu ClickUp API key en AppData para cargar listas reales.");
      }
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setStatus(`No se pudieron leer las API keys: ${message}`, "warning");
    }
  }

  function addVariable() {
    variables = [...variables, { id: Date.now(), key: "", value: "" }];
  }

  function removeVariable(id: number) {
    variables = variables.filter((variable) => variable.id !== id);
    generatedTask = null;
    setStatus("Variable eliminada de la plantilla.");
  }

  function normalizeVariable(variable: TemplateVariable) {
    variable.key = sanitizeVariableKey(variable.key);
    generatedTask = null;
  }

  function handleFiles(event: Event) {
    const input = event.currentTarget as HTMLInputElement;
    attachments = Array.from(input.files ?? []);
    setStatus(`${attachments.length} adjunto(s) listo(s) para enviar.`, attachments.length ? "success" : "neutral");
  }

  async function ensureApiKeysFile(shouldReveal: boolean) {
    isPreparingApiKeys = true;

    try {
      const resolvedPath = await invoke<string>("ensure_api_keys_file");
      apiKeysPath = resolvedPath;

      if (shouldReveal) {
        await revealItemInDir(resolvedPath);
      }
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setStatus(`No se pudo preparar el archivo de API keys: ${message}`, "warning");
    } finally {
      isPreparingApiKeys = false;
    }
  }

  async function openApiKeysAndReload() {
    await ensureApiKeysFile(true);
    await refreshApiState();
  }

  async function refreshClickUpLists() {
    if (!apiKeys.clickup_api_key.trim()) {
      setStatus("Configura la ClickUp API key primero.", "warning");
      return;
    }

    isLoadingLists = true;

    try {
      const lists = await invoke<ClickUpList[]>("list_clickup_lists");
      availableLists = lists;
      selectedListId = lists[0]?.id ?? "";
      setStatus(`Se cargaron ${lists.length} listas reales de ClickUp.`, "success");
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      availableLists = [];
      selectedListId = "";
      setStatus(`No se pudieron cargar las listas de ClickUp: ${message}`, "warning");
    } finally {
      isLoadingLists = false;
    }
  }

  async function generateWithGemini() {
    if (!template.trim()) {
      setStatus("Escribe el contenido de la tarea antes de usar Gemini.", "warning");
      return;
    }

    setStatus("La funcion de Gemini esta deshabilitada por el momento.", "warning");
    return;
  }

  async function buildAttachmentPayloads() {
    return Promise.all(
      attachments.map(async (file) => ({
        fileName: file.name,
        mimeType: file.type || null,
        bytes: Array.from(new Uint8Array(await file.arrayBuffer())),
      })),
    );
  }

  async function sendToClickUp() {
    if (!titleTemplate.trim()) {
      setStatus("Escribe el template del titulo antes de enviarla.", "warning");
      return;
    }

    if (!template.trim()) {
      setStatus("Escribe el contenido de la tarea antes de enviarla.", "warning");
      return;
    }

    if (!selectedListId) {
      setStatus("Selecciona una lista real de ClickUp.", "warning");
      return;
    }

    if (missingVariables.length) {
      setStatus(`Faltan valores para: ${missingVariables.join(", ")}.`, "warning");
      return;
    }

    isSending = true;

    try {
      const attachmentsPayload = await buildAttachmentPayloads();

      const response = await invoke<{ taskId: string; taskUrl: string | null; attachmentsUploaded: number }>(
        "create_clickup_task",
        {
          request: {
            listId: selectedListId,
            name: previewTitle,
            description: previewDescription,
            attachments: attachmentsPayload,
          },
        },
      );

      createdTaskUrl = response.taskUrl ?? "";
      setStatus(
        `Tarea creada en ${selectedList?.name ?? "ClickUp"} con ${response.attachmentsUploaded} adjunto(s).`,
        "success",
      );
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setStatus(`No se pudo crear la tarea en ClickUp: ${message}`, "warning");
    } finally {
      isSending = false;
    }
  }
</script>

<svelte:head>
  <title>Clikity</title>
  <meta name="color-scheme" content="dark" />
</svelte:head>

<main class="shell">
  <section class="workspace">
    <div class="editor-column">
      <article class="panel editor-panel">
        <header class="panel-header">
          <div>
            <p class="eyebrow">task_blueprint</p>
            <h2>Editor de tarea</h2>
          </div>
          <span class="chip">variables resaltadas</span>
        </header>

        <div class="title-builder">
          <div>
            <div class="section-label">Titulo de la tarea</div>
            <div class="title-surface">
              <div class="title-highlight" aria-hidden="true">{@html highlightedTitleTemplate + "\n"}</div>
              <input bind:value={titleTemplate} aria-label="Template del titulo" placeholder={"{{CLIENTE}} - {{SOLICITUD}}"} />
            </div>
          </div>
          <div class="title-preview-chip">
            <span>Resultado</span>
            <strong>{previewTitle}</strong>
          </div>
        </div>

        <div class="editor-split">
          <div class="editor-surface">
            <div class="editor-highlight" aria-hidden="true">
              {@html highlightedTemplate + "\n"}
            </div>

            <textarea
              bind:value={template}
              aria-label="Editor del template"
              spellcheck="false"
              placeholder={"Escribe aqui la tarea. Usa variables como {{CLIENTE}} o $PRIORIDAD"}
            ></textarea>
          </div>

          <div class="preview-panel">
            <div class="section-label">Preview final</div>
            <div class="preview-title">{previewTitle}</div>
            <pre>{previewDescription}</pre>
          </div>
        </div>
      </article>

      <div class="bottom-row">
        <article class="panel upload-panel">
          <header class="panel-header">
            <div>
              <p class="eyebrow">asset_ingestion</p>
              <h2>Adjuntos / imagenes</h2>
            </div>
          </header>

          <label class="dropzone">
            <input type="file" accept="image/*" multiple onchange={handleFiles} />
            <strong>Arrastra o selecciona una o multiples imagenes</strong>
            <span>Compatible con PNG, JPG, JPEG, WEBP y otros formatos soportados.</span>
          </label>

          <div class="attachments-grid">
            {#if attachments.length}
              {#each attachments as file}
                <article class="attachment-card">
                  <div class="thumb">IMG</div>
                  <div>
                    <strong>{file.name}</strong>
                    <span>{(file.size / 1024 / 1024).toFixed(2)} MB</span>
                  </div>
                </article>
              {/each}
            {:else}
              <p class="empty-state">Todavia no hay imagenes agregadas para la tarea.</p>
            {/if}
          </div>
        </article>

        <article class="panel actions-panel compact-actions">
          <header class="panel-header">
            <div>
              <p class="eyebrow">execution_core</p>
              <h2>Acciones</h2>
            </div>
          </header>

          <button class="action-card" type="button" onclick={openApiKeysAndReload} disabled={isPreparingApiKeys}>
            <span>API keys</span>
            <strong>{isPreparingApiKeys ? "Preparando archivo..." : "Abrir JSON y recargar"}</strong>
            <small>{apiKeysPath}</small>
          </button>

          <label class="select-card">
            <span>Lista de ClickUp</span>
            <select bind:value={selectedListId} disabled={!availableLists.length || isLoadingLists}>
              {#each availableLists as listItem}
                <option value={listItem.id}>{listItem.name}</option>
              {/each}
            </select>
            <small>{selectedList?.source ?? "Carga listas reales desde ClickUp"}</small>
          </label>

          <button class="secondary-button inline-button" type="button" onclick={refreshClickUpLists} disabled={!canLoadClickUpLists || isLoadingLists}>
            {isLoadingLists ? "Cargando listas..." : "Recargar listas ClickUp"}
          </button>

          <button class="primary-button" type="button" onclick={sendToClickUp} disabled={isSending}>
            {isSending ? "Preparando envio..." : "Enviar a ClickUp"}
          </button>

          <p class={`action-note ${statusTone}`}>{statusMessage}</p>

          {#if createdTaskUrl}
            <a class="task-link" href={createdTaskUrl} target="_blank" rel="noreferrer">Abrir tarea creada</a>
          {/if}
        </article>
      </div>
    </div>

    <aside class="sidebar">
      <article class="panel vars-panel">
        <header class="panel-header">
          <div>
            <p class="eyebrow">data_mapping</p>
            <h2>Variables</h2>
          </div>
        </header>

        <div class="detected-block">
          <div class="section-label">Detectadas en el texto</div>
          <div class="pill-row">
            {#if detectedVariables.length}
              {#each detectedVariables as variableName}
                <span class="pill">{variableName}</span>
              {/each}
            {:else}
              <span class="pill muted">Sin variables detectadas</span>
            {/if}
          </div>
        </div>

        <div class="variable-list">
          {#each variables as variable}
            <div class="variable-card">
              <input
                bind:value={variable.key}
                maxlength="40"
                placeholder="NOMBRE_VAR"
                oninput={() => normalizeVariable(variable)}
              />
              <textarea bind:value={variable.value} rows="2" placeholder="Valor de la variable"></textarea>
              <button class="ghost-button" type="button" onclick={() => removeVariable(variable.id)}>
                Eliminar
              </button>
            </div>
          {/each}
        </div>

        <button class="secondary-button" type="button" onclick={addVariable}>+ Agregar variable</button>
      </article>

    </aside>
  </section>

</main>

<style>
  :global(body) {
    margin: 0;
    min-width: 320px;
    min-height: 100vh;
    background: #17191f;
    color: #e7eaee;
    font-family: Inter, "Segoe UI", sans-serif;
    text-rendering: optimizeLegibility;
    -webkit-font-smoothing: antialiased;
    -moz-osx-font-smoothing: grayscale;
  }

  :global(*) {
    box-sizing: border-box;
  }

  :global(button),
  :global(input),
  :global(textarea),
  :global(select) {
    font: inherit;
  }

  h2,
  p,
  pre {
    margin: 0;
  }

  .shell {
    width: 100%;
    max-width: 1720px;
    margin: 0 auto;
    padding: 14px 18px;
  }

  .panel {
    background: #20242b;
    border: 1px solid #343942;
    border-radius: 14px;
    box-shadow: 0 10px 24px rgba(0, 0, 0, 0.18);
  }

  .eyebrow,
  .section-label,
  .action-card span,
  .select-card span,
  .attachments-grid span {
    color: #9aa3b2;
    text-transform: uppercase;
    letter-spacing: 0.16em;
    font-size: 0.68rem;
  }

  h2 {
    font-size: 0.98rem;
  }

  .empty-state,
  .action-card small {
    color: #9aa3b2;
  }

  .workspace {
    display: grid;
    grid-template-columns: minmax(0, 1.8fr) minmax(420px, 0.95fr);
    gap: 16px;
    align-items: start;
  }

  .editor-column,
  .sidebar {
    display: grid;
    gap: 16px;
  }

  .bottom-row {
    display: grid;
    grid-template-columns: minmax(0, 1.45fr) minmax(320px, 0.82fr);
    gap: 16px;
    align-items: stretch;
  }

  .panel-header {
    padding: 12px 14px 10px;
    border-bottom: 1px solid #31353d;
    display: flex;
    justify-content: space-between;
    gap: 8px;
    align-items: start;
  }

  .chip {
    background: rgba(97, 175, 239, 0.12);
    border: 1px solid rgba(97, 175, 239, 0.28);
    color: #9fd3ff;
    border-radius: 999px;
    padding: 4px 8px;
    font-size: 0.7rem;
    white-space: nowrap;
  }

  .editor-panel,
  .upload-panel,
  .vars-panel,
  .actions-panel {
    overflow: hidden;
  }

  .editor-panel {
    height: 500px;
    display: grid;
    grid-template-rows: auto auto 1fr;
  }

  .vars-panel {
    height: 766px;
    display: grid;
    grid-template-rows: auto auto 1fr auto;
  }

  .upload-panel {
    min-height: 380px;
    height: auto;
    display: grid;
    grid-template-rows: auto auto 1fr;
  }

  .editor-split {
    display: grid;
    grid-template-columns: minmax(0, 1.2fr) minmax(380px, 1fr);
    gap: 16px;
    padding: 12px 14px 14px;
    min-height: 0;
    height: 100%;
  }

  .title-builder {
    padding: 12px 14px 0;
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(260px, 320px);
    gap: 14px;
    align-items: center;
  }

  .title-surface {
    position: relative;
    min-height: 48px;
    border-radius: 12px;
    border: 1px solid #363b44;
    background: #13161b;
    overflow: hidden;
  }

  .title-highlight,
  .title-surface input {
    width: 100%;
    min-height: 48px;
    padding: 12px;
    font-family: "Cascadia Code", "Consolas", monospace;
    font-size: 0.9rem;
    line-height: 1.35;
    white-space: pre-wrap;
  }

  .title-highlight {
    pointer-events: none;
    color: #d9dde5;
  }

  .title-highlight :global(.token) {
    background: rgba(97, 175, 239, 0.16);
    color: #7fc4ff;
    border-radius: 6px;
    padding: 1px 4px;
  }

  .title-surface input {
    position: absolute;
    inset: 0;
    border: 0;
    outline: none;
    background: transparent;
    color: transparent;
    caret-color: #f4f7fb;
  }

  .title-preview-chip {
    min-height: 48px;
    padding: 10px 12px;
    border-radius: 12px;
    border: 1px solid #31353d;
    background: #171a20;
    display: grid;
    gap: 4px;
  }

  .title-preview-chip span {
    color: #9aa3b2;
    text-transform: uppercase;
    letter-spacing: 0.16em;
    font-size: 0.68rem;
  }

  .title-preview-chip strong {
    color: #f2f5f8;
    font-size: 0.92rem;
    word-break: break-word;
  }

  .editor-surface {
    position: relative;
    min-height: 0;
    height: 100%;
    border-radius: 12px;
    border: 1px solid #363b44;
    background: #13161b;
    overflow: hidden;
  }

  .editor-highlight,
  .editor-surface textarea {
    margin: 0;
    padding: 12px;
    width: 100%;
    min-height: 0;
    height: 100%;
    font-family: "Cascadia Code", "Consolas", monospace;
    font-size: 0.9rem;
    line-height: 1.45;
    white-space: pre-wrap;
    word-break: break-word;
  }

  .editor-highlight {
    pointer-events: none;
    color: #d9dde5;
  }

  .editor-highlight :global(.token) {
    background: rgba(97, 175, 239, 0.16);
    color: #7fc4ff;
    border-radius: 6px;
    padding: 1px 4px;
  }

  .editor-surface textarea {
    position: absolute;
    inset: 0;
    resize: none;
    border: 0;
    outline: none;
    background: transparent;
    color: transparent;
    caret-color: #f4f7fb;
  }

  .editor-surface textarea::selection {
    background: rgba(97, 175, 239, 0.35);
  }

  .preview-panel,
  .detected-block {
    padding: 12px;
    border-radius: 12px;
    background: #171a20;
    border: 1px solid #31353d;
  }

  .preview-panel {
    min-height: 0;
    height: 100%;
    overflow: auto;
  }

  .preview-panel pre {
    margin-top: 8px;
    color: #d9dde5;
    white-space: pre-wrap;
    line-height: 1.45;
    font-size: 0.9rem;
  }

  .preview-title {
    margin-top: 8px;
    font-size: 1.1rem;
    font-weight: 700;
    color: #f2f5f8;
  }

  .dropzone {
    margin: 12px 14px;
    min-height: 120px;
    display: grid;
    place-items: center;
    gap: 6px;
    border: 1px dashed #49505d;
    border-radius: 12px;
    background: #171a20;
    text-align: center;
    padding: 14px;
    cursor: pointer;
  }

  .dropzone input {
    display: none;
  }

  .dropzone strong {
    font-size: 0.94rem;
  }

  .dropzone span {
    color: #a5aebb;
    line-height: 1.35;
    max-width: 420px;
    font-size: 0.84rem;
  }

  .attachments-grid {
    margin: 0 14px 14px;
    display: grid;
    gap: 8px;
  }

  .attachment-card {
    display: grid;
    grid-template-columns: 42px 1fr;
    gap: 10px;
    align-items: center;
    padding: 10px;
    border-radius: 12px;
    border: 1px solid #31353d;
    background: #171a20;
  }

  .thumb {
    width: 42px;
    height: 42px;
    border-radius: 10px;
    display: grid;
    place-items: center;
    background: #111319;
    border: 1px solid #363b44;
    color: #7fc4ff;
    font-size: 0.75rem;
    letter-spacing: 0.1em;
  }

  .attachment-card strong,
  .action-card strong {
    display: block;
    margin-bottom: 4px;
  }

  .pill-row,
  .variable-list {
    display: grid;
    gap: 8px;
  }

  .pill-row {
    margin-top: 8px;
    grid-template-columns: repeat(auto-fit, minmax(96px, 1fr));
  }

  .pill {
    padding: 6px 8px;
    border-radius: 10px;
    background: rgba(97, 175, 239, 0.12);
    border: 1px solid rgba(97, 175, 239, 0.24);
    color: #b8ddfb;
    text-align: center;
    font-size: 0.8rem;
  }

  .pill.muted {
    background: #171a20;
    border-color: #31353d;
    color: #8d97a8;
  }

  .variable-list {
    padding: 0 14px 14px;
    max-height: 100%;
    overflow: auto;
    scrollbar-gutter: stable;
  }

  .variable-card {
    display: grid;
    gap: 8px;
    padding: 10px;
    border-radius: 12px;
    background: #171a20;
    border: 1px solid #31353d;
  }

  input,
  select,
  .variable-card textarea {
    width: 100%;
    border: 1px solid #3b414b;
    background: #101319;
    color: #eef1f5;
    border-radius: 10px;
    padding: 9px 11px;
    outline: none;
  }

  .variable-card textarea {
    position: static;
    min-height: 58px;
    resize: vertical;
    color: #eef1f5;
    caret-color: #eef1f5;
    white-space: pre-wrap;
  }

  input:focus,
  select:focus,
  .variable-card textarea:focus {
    border-color: #61afef;
    box-shadow: 0 0 0 3px rgba(97, 175, 239, 0.15);
  }

  .secondary-button,
  .ghost-button,
  .primary-button,
  .action-card {
    border: 0;
    cursor: pointer;
    transition: 160ms ease;
  }

  .secondary-button,
  .primary-button {
    margin: 0 14px 14px;
    padding: 11px 14px;
    border-radius: 12px;
    font-weight: 700;
  }

  .secondary-button {
    background: #2b313a;
    color: #eef2f7;
    border: 1px solid #3a414c;
  }

  .secondary-button:hover,
  .ghost-button:hover,
  .action-card:hover,
  .primary-button:hover {
    transform: translateY(-1px);
  }

  .ghost-button {
    justify-self: start;
    padding: 8px 10px;
    border-radius: 10px;
    background: rgba(255, 255, 255, 0.04);
    color: #d4d9e0;
  }

  .actions-panel {
    display: grid;
    gap: 10px;
  }

  .compact-actions {
    align-self: stretch;
    min-height: 380px;
    height: auto;
    grid-template-rows: auto auto auto auto auto auto 1fr auto;
  }

  .compact-actions .primary-button {
    align-self: end;
  }

  .inline-button {
    margin-top: -2px;
  }

  .action-card,
  .select-card {
    margin: 0 14px;
    padding: 12px;
    border-radius: 12px;
    background: #171a20;
    border: 1px solid #31353d;
    text-align: left;
    overflow-wrap: anywhere;
  }

  .action-card {
    color: inherit;
  }

  .action-card:disabled,
  .primary-button:disabled {
    opacity: 0.65;
    cursor: wait;
    transform: none;
  }

  .select-card {
    display: grid;
    gap: 8px;
  }

  .primary-button {
    background: #61afef;
    color: #08111a;
  }

  .action-note {
    margin: 0 14px;
    font-size: 0.82rem;
    line-height: 1.4;
    color: #aeb6c3;
  }

  .action-note.success {
    color: #8dd6a5;
  }

  .action-note.warning {
    color: #f0c27a;
  }

  .task-link {
    margin: 0 14px 14px;
    color: #7fc4ff;
    text-decoration: none;
    font-size: 0.88rem;
    font-weight: 600;
  }

  .task-link:hover {
    text-decoration: underline;
  }

  @media (max-width: 980px), (max-height: 760px) {
    .workspace {
      grid-template-columns: 1fr;
    }

    .bottom-row {
      grid-template-columns: 1fr;
    }

    .title-builder,
    .editor-split {
      grid-template-columns: 1fr;
    }

    .sidebar {
      grid-template-columns: 1fr 1fr;
    }

    .editor-surface,
    .editor-highlight,
    .editor-surface textarea,
    .preview-panel {
      min-height: 200px;
      height: 200px;
    }

    .editor-panel {
      height: auto;
    }

    .vars-panel,
    .upload-panel,
    .compact-actions {
      height: auto;
      min-height: 0;
    }

    .vars-panel {
      grid-template-rows: auto auto 1fr auto;
    }

    .variable-list {
      max-height: 240px;
    }
  }

  @media (max-width: 700px), (max-height: 620px) {
    .shell {
      padding: 8px;
    }

    .panel {
      border-radius: 10px;
    }

    .sidebar {
      grid-template-columns: 1fr;
    }

    .bottom-row {
      grid-template-columns: 1fr;
    }

    .chip {
      display: none;
    }

    .title-builder {
      grid-template-columns: 1fr;
    }

    .editor-surface,
    .editor-highlight,
    .editor-surface textarea,
    .preview-panel {
      min-height: 168px;
      height: 168px;
      font-size: 0.84rem;
    }

    .editor-panel {
      height: auto;
    }

    .vars-panel,
    .upload-panel,
    .compact-actions {
      height: auto;
      min-height: 0;
    }

    .dropzone {
      min-height: 86px;
    }

    .variable-list {
      max-height: 180px;
    }
  }
</style>
