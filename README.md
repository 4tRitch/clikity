# Clikity

App de escritorio para Windows hecha con **Tauri + SvelteKit + TypeScript** para crear tareas de ClickUp a partir de una plantilla con variables y adjuntos.

## Que hace

- arma el titulo y la descripcion desde una plantilla editable
- resalta variables detectadas en el texto
- permite cargar varias imagenes como adjuntos
- obtiene listas reales de ClickUp desde tu cuenta
- envia la tarea final a ClickUp

## Regla importante sobre imagenes

Las imagenes **siempre se colocan al final de toda la tarea**.

Clikity mantiene la implementacion y el resto del contenido en texto limpio, y luego agrega una seccion final:

```md
---

## Imagenes adjuntas

![captura-1](...)

![captura-2](...)
```

Si la descripcion ya trae una seccion previa de imagenes, la app la reconstruye para evitar duplicados o posiciones intermedias.

## Desarrollo local

```bash
npm install
npm run check
npm run build
```

Para abrir la app de escritorio:

```bash
npm run tauri dev
```

## API keys

La app crea y usa este archivo en Windows:

```text
%APPDATA%\com.cruemy.clikity\api-keys.json
```

Formato esperado:

```json
{
  "clickup_api_key": "",
  "gemini_api_key": ""
}
```

## Validaciones realizadas

Validacion tecnica local:

- `cargo test`
- `npm run check`
- `npm run build`

Pruebas reales en ClickUp:

- creacion de una tarea sin adjuntos
- creacion de una tarea con dos imagenes adjuntas y seccion `Imagenes adjuntas` al final

## Stack recomendado

- VS Code
- extension Svelte
- extension Tauri
- rust-analyzer
