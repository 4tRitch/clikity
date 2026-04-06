use std::{collections::HashSet, fs, path::PathBuf};

use reqwest::{multipart, Client};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::Manager;

const CLICKUP_API_BASE: &str = "https://api.clickup.com/api/v2";
const GEMINI_API_BASE: &str =
    "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.0-flash:generateContent";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ApiKeysFile {
    clickup_api_key: String,
    gemini_api_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TemplateVariableInput {
    key: String,
    value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiGenerateRequest {
    template: String,
    rendered_text: String,
    variables: Vec<TemplateVariableInput>,
    attachment_names: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GeneratedTask {
    title: String,
    description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ClickUpListOption {
    id: String,
    name: String,
    source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AttachmentUploadInput {
    file_name: String,
    mime_type: Option<String>,
    bytes: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateClickUpTaskRequest {
    list_id: String,
    name: String,
    description: String,
    attachments: Vec<AttachmentUploadInput>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateClickUpTaskResponse {
    task_id: String,
    task_url: Option<String>,
    attachments_uploaded: usize,
}

#[derive(Debug, Deserialize)]
struct ClickUpTeamsResponse {
    teams: Vec<ClickUpTeam>,
}

#[derive(Debug, Deserialize)]
struct ClickUpTeam {
    id: String,
    name: String,
}

#[derive(Debug, Deserialize)]
struct ClickUpSpacesResponse {
    spaces: Vec<ClickUpSpace>,
}

#[derive(Debug, Deserialize)]
struct ClickUpSpace {
    id: String,
    name: String,
}

#[derive(Debug, Deserialize)]
struct ClickUpFoldersResponse {
    folders: Vec<ClickUpFolder>,
}

#[derive(Debug, Deserialize)]
struct ClickUpFolder {
    id: String,
    name: String,
}

#[derive(Debug, Deserialize)]
struct ClickUpListsResponse {
    lists: Vec<ClickUpListEntity>,
}

#[derive(Debug, Deserialize)]
struct ClickUpListEntity {
    id: String,
    name: String,
}

#[derive(Debug, Deserialize)]
struct ClickUpCreateTaskResult {
    id: String,
    url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ClickUpAttachmentUploadResult {
    url: Option<String>,
    thumbnail_url: Option<String>,
    title: Option<String>,
}

fn build_markdown_with_images(description: &str, image_urls: &[(String, String)]) -> String {
    if image_urls.is_empty() {
        return description.to_string();
    }

    let base_description = strip_existing_images_section(description);

    let images_block = image_urls
        .iter()
        .map(|(name, url)| format!("![{}]({})", name, url))
        .collect::<Vec<_>>()
        .join("\n\n");

    format!(
        "{description}\n\n---\n\n## Imagenes adjuntas\n\n{images_block}",
        description = base_description,
        images_block = images_block
    )
}

fn strip_existing_images_section(description: &str) -> String {
    let trimmed = description.trim();
    let marker = "## Imagenes adjuntas";

    if let Some(index) = trimmed.find(marker) {
        return trimmed[..index].trim_end_matches('-').trim().to_string();
    }

    trimmed.to_string()
}

fn api_keys_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("No se pudo resolver AppData: {error}"))?;

    fs::create_dir_all(&app_data_dir)
        .map_err(|error| format!("No se pudo crear el directorio AppData: {error}"))?;

    Ok(app_data_dir.join("api-keys.json"))
}

fn ensure_keys_file_exists(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let file_path = api_keys_path(app)?;

    if !file_path.exists() {
        let template = serde_json::to_string_pretty(&ApiKeysFile::default())
            .map_err(|error| format!("No se pudo serializar el template de API keys: {error}"))?;

        fs::write(&file_path, format!("{template}\n"))
            .map_err(|error| format!("No se pudo crear api-keys.json: {error}"))?;
    }

    Ok(file_path)
}

fn read_api_keys(app: &tauri::AppHandle) -> Result<ApiKeysFile, String> {
    let file_path = ensure_keys_file_exists(app)?;
    let content = fs::read_to_string(&file_path)
        .map_err(|error| format!("No se pudo leer api-keys.json: {error}"))?;

    serde_json::from_str::<ApiKeysFile>(&content)
        .map_err(|error| format!("api-keys.json no tiene un formato JSON valido: {error}"))
}

fn write_api_keys(app: &tauri::AppHandle, keys: &ApiKeysFile) -> Result<(), String> {
    let file_path = ensure_keys_file_exists(app)?;
    let content = serde_json::to_string_pretty(keys)
        .map_err(|error| format!("No se pudo serializar api-keys.json: {error}"))?;

    fs::write(&file_path, format!("{content}\n"))
        .map_err(|error| format!("No se pudo guardar api-keys.json: {error}"))
}

fn require_clickup_key(app: &tauri::AppHandle) -> Result<String, String> {
    let keys = read_api_keys(app)?;
    let key = keys.clickup_api_key.trim().to_string();

    if key.is_empty() {
        return Err("La ClickUp API key esta vacia en api-keys.json".to_string());
    }

    Ok(key)
}

fn require_gemini_key(app: &tauri::AppHandle) -> Result<String, String> {
    let keys = read_api_keys(app)?;
    let key = keys.gemini_api_key.trim().to_string();

    if key.is_empty() {
        return Err("La Gemini API key esta vacia en api-keys.json".to_string());
    }

    Ok(key)
}

async fn clickup_get<T: for<'de> Deserialize<'de>>(
    client: &Client,
    token: &str,
    url: &str,
) -> Result<T, String> {
    let response = client
        .get(url)
        .header("Authorization", token)
        .send()
        .await
        .map_err(|error| format!("Error de red con ClickUp: {error}"))?;

    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|error| format!("No se pudo leer la respuesta de ClickUp: {error}"))?;

    if !status.is_success() {
        return Err(format!("ClickUp respondio {status}: {body}"));
    }

    serde_json::from_str::<T>(&body)
        .map_err(|error| format!("No se pudo parsear la respuesta de ClickUp: {error}"))
}

fn extract_text_from_gemini_response(body: &Value) -> Result<String, String> {
    body.get("candidates")
        .and_then(Value::as_array)
        .and_then(|candidates| candidates.first())
        .and_then(|candidate| candidate.get("content"))
        .and_then(|content| content.get("parts"))
        .and_then(Value::as_array)
        .and_then(|parts| parts.first())
        .and_then(|part| part.get("text"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .ok_or_else(|| "Gemini no devolvio contenido utilizable".to_string())
}

#[tauri::command]
fn ensure_api_keys_file(app: tauri::AppHandle) -> Result<String, String> {
    ensure_keys_file_exists(&app)?
        .into_os_string()
        .into_string()
        .map_err(|_| "La ruta del archivo contiene caracteres no validos".to_string())
}

#[tauri::command]
fn load_api_keys(app: tauri::AppHandle) -> Result<ApiKeysFile, String> {
    read_api_keys(&app)
}

#[tauri::command]
fn save_api_keys(app: tauri::AppHandle, keys: ApiKeysFile) -> Result<(), String> {
    write_api_keys(&app, &keys)
}

#[tauri::command]
async fn list_clickup_lists(app: tauri::AppHandle) -> Result<Vec<ClickUpListOption>, String> {
    let token = require_clickup_key(&app)?;
    let client = Client::new();
    let teams: ClickUpTeamsResponse = clickup_get(&client, &token, &format!("{CLICKUP_API_BASE}/team")).await?;

    let mut seen = HashSet::new();
    let mut results = Vec::new();

    for team in teams.teams {
        let spaces: ClickUpSpacesResponse = clickup_get(
            &client,
            &token,
            &format!("{CLICKUP_API_BASE}/team/{}/space?archived=false", team.id),
        )
        .await?;

        for space in spaces.spaces {
            let direct_lists: ClickUpListsResponse = clickup_get(
                &client,
                &token,
                &format!("{CLICKUP_API_BASE}/space/{}/list?archived=false", space.id),
            )
            .await?;

            for list in direct_lists.lists {
                if seen.insert(list.id.clone()) {
                    results.push(ClickUpListOption {
                        id: list.id,
                        name: list.name,
                        source: format!("{} / {}", team.name, space.name),
                    });
                }
            }

            let folders: ClickUpFoldersResponse = clickup_get(
                &client,
                &token,
                &format!("{CLICKUP_API_BASE}/space/{}/folder?archived=false", space.id),
            )
            .await?;

            for folder in folders.folders {
                let folder_lists: ClickUpListsResponse = clickup_get(
                    &client,
                    &token,
                    &format!("{CLICKUP_API_BASE}/folder/{}/list?archived=false", folder.id),
                )
                .await?;

                for list in folder_lists.lists {
                    if seen.insert(list.id.clone()) {
                        results.push(ClickUpListOption {
                            id: list.id,
                            name: list.name,
                            source: format!("{} / {} / {}", team.name, space.name, folder.name),
                        });
                    }
                }
            }
        }
    }

    results.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

    Ok(results)
}

#[tauri::command]
async fn generate_task_with_gemini(
    app: tauri::AppHandle,
    request: GeminiGenerateRequest,
) -> Result<GeneratedTask, String> {
    let api_key = require_gemini_key(&app)?;
    let client = Client::new();

    let variables_text = request
        .variables
        .iter()
        .map(|variable| format!("- {} = {}", variable.key, variable.value))
        .collect::<Vec<_>>()
        .join("\n");

    let attachments_text = if request.attachment_names.is_empty() {
        "- Sin adjuntos".to_string()
    } else {
        request
            .attachment_names
            .iter()
            .map(|name| format!("- {name}"))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let prompt = format!(
        "Genera un JSON con las claves title y description para crear una tarea profesional en ClickUp. \
El idioma debe ser espanol. title debe ser corto (maximo 80 caracteres) y description debe ser clara, accionable y bien estructurada en texto plano. Si hay imagenes o adjuntos, no los mezcles dentro de la implementacion: la app los agregara automaticamente al final de toda la tarea en una seccion separada.\n\nTemplate original:\n{template}\n\nTexto renderizado:\n{rendered}\n\nVariables:\n{variables}\n\nAdjuntos:\n{attachments}",
        template = request.template,
        rendered = request.rendered_text,
        variables = variables_text,
        attachments = attachments_text,
    );

    let response = client
        .post(GEMINI_API_BASE)
        .header("x-goog-api-key", api_key)
        .json(&json!({
            "contents": [{
                "parts": [{ "text": prompt }]
            }],
            "generationConfig": {
                "responseMimeType": "application/json",
                "responseJsonSchema": {
                    "type": "object",
                    "properties": {
                        "title": { "type": "string" },
                        "description": { "type": "string" }
                    },
                    "required": ["title", "description"]
                }
            }
        }))
        .send()
        .await
        .map_err(|error| format!("Error al llamar a Gemini: {error}"))?;

    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|error| format!("No se pudo leer la respuesta de Gemini: {error}"))?;

    if !status.is_success() {
        return Err(format!("Gemini respondio {status}: {body}"));
    }

    let parsed_body: Value =
        serde_json::from_str(&body).map_err(|error| format!("Gemini devolvio JSON invalido: {error}"))?;
    let text = extract_text_from_gemini_response(&parsed_body)?;

    serde_json::from_str::<GeneratedTask>(&text)
        .map_err(|error| format!("No se pudo parsear el JSON generado por Gemini: {error}"))
}

#[tauri::command]
async fn create_clickup_task(
    app: tauri::AppHandle,
    request: CreateClickUpTaskRequest,
) -> Result<CreateClickUpTaskResponse, String> {
    let token = require_clickup_key(&app)?;
    let client = Client::new();

    let original_description = request.description.clone();

    let create_response = client
        .post(format!("{CLICKUP_API_BASE}/list/{}/task", request.list_id))
        .header("Authorization", token.clone())
        .json(&json!({
            "name": request.name,
            "description": original_description,
            "markdown_description": original_description,
        }))
        .send()
        .await
        .map_err(|error| format!("Error al crear la tarea en ClickUp: {error}"))?;

    let create_status = create_response.status();
    let create_body = create_response
        .text()
        .await
        .map_err(|error| format!("No se pudo leer la respuesta de crear tarea: {error}"))?;

    if !create_status.is_success() {
        return Err(format!("ClickUp no creo la tarea ({create_status}): {create_body}"));
    }

    let task: ClickUpCreateTaskResult = serde_json::from_str(&create_body)
        .map_err(|error| format!("No se pudo parsear la respuesta de crear tarea: {error}"))?;

    let mut attachments_uploaded = 0usize;
    let mut uploaded_images: Vec<(String, String)> = Vec::new();

    for attachment in request.attachments {
        let AttachmentUploadInput {
            file_name,
            mime_type,
            bytes,
        } = attachment;

        let part = match mime_type.as_deref() {
            Some(mime_type) => multipart::Part::bytes(bytes.clone())
                .file_name(file_name.clone())
                .mime_str(mime_type)
                .unwrap_or_else(|_| multipart::Part::bytes(bytes).file_name(file_name.clone())),
            None => multipart::Part::bytes(bytes).file_name(file_name.clone()),
        };

        let form = multipart::Form::new().part("attachment", part);
        let response = client
            .post(format!("{CLICKUP_API_BASE}/task/{}/attachment", task.id))
            .header("Authorization", token.clone())
            .multipart(form)
            .send()
            .await
            .map_err(|error| format!("Error al subir adjunto a ClickUp: {error}"))?;

        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|error| format!("No se pudo leer la respuesta del adjunto: {error}"))?;

        if !status.is_success() {
            return Err(format!("ClickUp no pudo subir un adjunto: {body}"));
        }

        let upload_result: ClickUpAttachmentUploadResult = serde_json::from_str(&body)
            .map_err(|error| format!("No se pudo parsear la respuesta del adjunto: {error}"))?;

        let file_name_for_markdown = upload_result.title.unwrap_or(file_name);
        if let Some(url) = upload_result.url.or(upload_result.thumbnail_url) {
            uploaded_images.push((file_name_for_markdown, url));
        }

        attachments_uploaded += 1;
    }

    if !uploaded_images.is_empty() {
        let markdown_content = build_markdown_with_images(&original_description, &uploaded_images);

        let update_response = client
            .put(format!("{CLICKUP_API_BASE}/task/{}", task.id))
            .header("Authorization", token.clone())
            .json(&json!({
                "markdown_content": markdown_content,
            }))
            .send()
            .await
            .map_err(|error| format!("Error al actualizar la descripcion con imagenes: {error}"))?;

        let update_status = update_response.status();
        let update_body = update_response
            .text()
            .await
            .map_err(|error| format!("No se pudo leer la respuesta de actualizacion: {error}"))?;

        if !update_status.is_success() {
            return Err(format!(
                "La tarea se creo pero no se pudo actualizar la descripcion con imagenes ({update_status}): {update_body}"
            ));
        }
    }

    Ok(CreateClickUpTaskResponse {
        task_id: task.id,
        task_url: task.url,
        attachments_uploaded,
    })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            ensure_api_keys_file,
            load_api_keys,
            save_api_keys,
            list_clickup_lists,
            generate_task_with_gemini,
            create_clickup_task,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn repo_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root")
            .to_path_buf()
    }

    #[test]
    fn normal_task_request_is_valid_without_attachments() {
        let request = CreateClickUpTaskRequest {
            list_id: "123".to_string(),
            name: "Tarea normal de prueba".to_string(),
            description: "Descripcion simple para validar el flujo base.".to_string(),
            attachments: vec![],
        };

        assert_eq!(request.list_id, "123");
        assert_eq!(request.attachments.len(), 0);
        assert!(request.description.contains("flujo base"));
    }

    #[test]
    fn image_task_request_uses_project_dummy_pngs() {
        let root = repo_root();
        let dummy_files = ["aa.png", "ad.png", "ss.png"];

        let attachments = dummy_files
            .iter()
            .map(|file_name| {
                let bytes = fs::read(root.join(file_name)).expect("dummy image should exist");

                AttachmentUploadInput {
                    file_name: (*file_name).to_string(),
                    mime_type: Some("image/png".to_string()),
                    bytes,
                }
            })
            .collect::<Vec<_>>();

        let request = CreateClickUpTaskRequest {
            list_id: "456".to_string(),
            name: "Tarea con imagenes".to_string(),
            description: "Prueba con adjuntos dummy del proyecto.".to_string(),
            attachments,
        };

        assert_eq!(request.attachments.len(), 3);
        assert!(request.attachments.iter().all(|item| !item.bytes.is_empty()));
        assert_eq!(request.attachments[0].file_name, "aa.png");
    }

    #[test]
    fn gemini_response_text_is_extracted() {
        let payload = json!({
            "candidates": [{
                "content": {
                    "parts": [{
                        "text": "{\"title\":\"Demo\",\"description\":\"Texto\"}"
                    }]
                }
            }]
        });

        let extracted = extract_text_from_gemini_response(&payload).expect("text should be extracted");
        assert!(extracted.contains("title"));
        assert!(extracted.contains("description"));
    }

    #[test]
    fn markdown_description_includes_embedded_images() {
        let markdown = build_markdown_with_images(
            "Descripcion base",
            &[
                ("aa.png".to_string(), "https://files.example/aa.png".to_string()),
                ("ad.png".to_string(), "https://files.example/ad.png".to_string()),
            ],
        );

        assert!(markdown.contains("## Imagenes adjuntas"));
        assert!(markdown.contains("![aa.png](https://files.example/aa.png)"));
        assert!(markdown.contains("![ad.png](https://files.example/ad.png)"));
    }

    #[test]
    fn markdown_images_are_appended_after_realistic_implementation_sections() {
        let description = "# Objetivo\nPreparar el alta del cliente en ClickUp.\n\n## Implementacion\n1. Validar datos del cliente.\n2. Crear la tarea en la lista operativa.\n3. Confirmar el resultado con operaciones.\n\n## Resultado esperado\n- La tarea queda lista para seguimiento.";

        let markdown = build_markdown_with_images(
            description,
            &[("evidencia-final.png".to_string(), "https://files.example/evidencia-final.png".to_string())],
        );

        let implementation_index = markdown.find("## Implementacion").expect("implementation section should exist");
        let expected_result_index = markdown.find("## Resultado esperado").expect("expected result section should exist");
        let images_index = markdown.find("## Imagenes adjuntas").expect("images section should exist");

        assert!(implementation_index < expected_result_index);
        assert!(expected_result_index < images_index);
        assert!(markdown.ends_with("![evidencia-final.png](https://files.example/evidencia-final.png)"));
    }

    #[test]
    fn markdown_replaces_existing_image_section_and_keeps_it_at_the_end() {
        let description = "Resumen de la tarea.\n\n## Implementacion\n1. Abrir el modal.\n\n## Imagenes adjuntas\n\n![vieja.png](https://files.example/vieja.png)\n\n## Notas\n- Este bloque no debe sobrevivir despues de mover imagenes al final.";

        let markdown = build_markdown_with_images(
            description,
            &[("nueva.png".to_string(), "https://files.example/nueva.png".to_string())],
        );

        assert_eq!(markdown.matches("## Imagenes adjuntas").count(), 1);
        assert!(!markdown.contains("vieja.png"));
        assert!(markdown.ends_with("![nueva.png](https://files.example/nueva.png)"));
        assert!(!markdown.contains("## Notas"));
    }
}
