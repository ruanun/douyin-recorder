// Transport bridge: the same stores/components run in two deployment shapes.
//
// - Desktop (Tauri): IPC via `invoke` / `listen`, files opened with the OS
//   opener plugin.
// - Web (headless server): REST via `POST /api/invoke/{command}`, events via
//   Server-Sent Events, files downloaded through `/api/download`.
//
// Detection is based on the Tauri runtime marker injected into the webview.

import type { LifecycleStatus } from '../types'

export const isTauriRuntime = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window

const TOKEN_STORAGE_KEY = 'douyin-recorder-token'

function serverToken(): string {
  if (typeof window === 'undefined') return ''
  // A token may be supplied as ?token=... (then remembered) for first access.
  const url = new URL(window.location.href)
  const fromUrl = url.searchParams.get('token')
  if (fromUrl) {
    localStorage.setItem(TOKEN_STORAGE_KEY, fromUrl)
    url.searchParams.delete('token')
    window.history.replaceState({}, '', url.toString())
    return fromUrl
  }
  return localStorage.getItem(TOKEN_STORAGE_KEY) || ''
}

/** Ask the user for the access token (browser mode) after a 401. */
let promptingForToken = false
async function promptForToken(): Promise<void> {
  if (promptingForToken) return
  promptingForToken = true
  try {
    const { ElMessageBox } = await import('element-plus')
    const { value } = await ElMessageBox.prompt(
      '服务器的接口需要访问令牌。请输入部署时设置的 DOUYIN_RECORDER_TOKEN。',
      '需要访问令牌',
      {
        confirmButtonText: '确定',
        cancelButtonText: '取消',
        inputPlaceholder: '访问令牌',
        inputType: 'password',
        inputValidator: (input: string) => (input && input.trim() ? true : '令牌不能为空'),
      },
    )
    localStorage.setItem(TOKEN_STORAGE_KEY, value.trim())
    window.location.reload()
  } catch {
    // Cancelled: leave the app showing empty data until the user retries.
  } finally {
    promptingForToken = false
  }
}

export function withToken(url: string): string {
  const token = serverToken()
  if (!token) return url
  return `${url}${url.includes('?') ? '&' : '?'}token=${encodeURIComponent(token)}`
}

function apiUrl(path: string): string {
  return withToken(path)
}

/** Invoke a backend command (desktop IPC or HTTP JSON-RPC style call). */
export async function invokeCommand<T>(command: string, args: Record<string, unknown> = {}): Promise<T> {
  if (isTauriRuntime) {
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<T>(command, args)
  }
  const response = await fetch(apiUrl(`/api/invoke/${encodeURIComponent(command)}`), {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      ...(serverToken() ? { 'X-Auth-Token': serverToken() } : {}),
    },
    body: JSON.stringify(args),
  })
  const text = await response.text()
  let payload: unknown = null
  try {
    payload = text ? JSON.parse(text) : null
  } catch {
    payload = text
  }
  if (response.status === 401) {
    localStorage.removeItem(TOKEN_STORAGE_KEY)
    void promptForToken()
    throw new Error('需要访问令牌')
  }
  if (!response.ok) {
    const message = typeof payload === 'object' && payload && 'error' in payload
      ? String((payload as { error: unknown }).error)
      : String(payload || response.statusText)
    throw new Error(message)
  }
  return payload as T
}

/**
 * Shared event channel. All event types multiplex over a single SSE connection:
 * browsers cap concurrent HTTP/1.1 connections per origin (6), so one
 * EventSource per event type would starve ordinary requests and freeze the UI.
 */
type EventHandler = (payload: unknown) => void
let sharedEventSource: EventSource | null = null
const eventHandlers = new Map<string, Set<EventHandler>>()

function ensureEventSource(): EventSource {
  if (sharedEventSource) return sharedEventSource
  const source = new EventSource(apiUrl('/api/events'))
  // EventSource reconnects automatically; handlers stay registered.
  source.onerror = () => undefined
  sharedEventSource = source
  return source
}

/**
 * Subscribe to a backend event. Returns an unsubscribe function.
 * Event names are identical in both transports (`-` is mapped to `_` for
 * SSE event names because SSE identifiers cannot contain dashes safely).
 */
export async function onEvent<T>(event: string, handler: (payload: T) => void): Promise<() => void> {
  if (isTauriRuntime) {
    const { listen } = await import('@tauri-apps/api/event')
    return listen<T>(event, ({ payload }) => handler(payload))
  }
  const source = ensureEventSource()
  const sseName = event.split('-').join('_')
  let registered = eventHandlers.get(sseName)
  if (!registered) {
    registered = new Set()
    eventHandlers.set(sseName, registered)
    source.addEventListener(sseName, (message) => {
      let payload: unknown
      try {
        payload = JSON.parse((message as MessageEvent).data)
      } catch (error) {
        console.error(`解析事件 ${event} 失败:`, error)
        return
      }
      for (const listener of registered!) listener(payload)
    })
  }
  const listener = handler as EventHandler
  registered.add(listener)
  return () => {
    eventHandlers.get(sseName)?.delete(listener)
  }
}

/** Lifecycle status is a desktop-only concept; the web server is always running. */
export async function getLifecycleStatus(): Promise<LifecycleStatus> {
  if (isTauriRuntime) {
    return invokeCommand<LifecycleStatus>('get_lifecycle_status')
  }
  return { exiting: false, revision: 0, message: null }
}

export function isDesktop(): boolean {
  return isTauriRuntime
}

/** Open a recorded file with the OS handler (desktop only: the web build has no downloads). */
export async function openRecordedFile(options: { path: string }): Promise<void> {
  if (!isTauriRuntime) {
    throw new Error('Web 模式不提供文件下载，请在服务器的录制目录中查看')
  }
  const { openPath } = await import('@tauri-apps/plugin-opener')
  await openPath(options.path)
}

/** Reveal a file in the OS file manager (desktop only). */
export async function revealRecordedFile(options: { path: string }): Promise<void> {
  if (!isTauriRuntime) {
    throw new Error('Web 模式不提供文件夹操作，请在服务器的录制目录中查看')
  }
  const { revealItemInDir } = await import('@tauri-apps/plugin-opener')
  await revealItemInDir(options.path)
}

/** Open the recordings/logs directory. */
export async function openDirectory(path: string): Promise<void> {
  if (isTauriRuntime) {
    const { openPath } = await import('@tauri-apps/plugin-opener')
    await openPath(path)
    return
  }
  throw new Error(`浏览器模式无法打开目录，路径：${path}`)
}

/** Open an external URL (update page, project home, …). */
export async function openExternalUrl(url: string): Promise<void> {
  if (isTauriRuntime) {
    const { openUrl } = await import('@tauri-apps/plugin-opener')
    await openUrl(url)
    return
  }
  window.open(url, '_blank', 'noopener')
}

/** App version shown in the settings header. */
export async function getAppVersion(): Promise<string> {
  if (isTauriRuntime) {
    const { getVersion } = await import('@tauri-apps/api/app')
    return getVersion()
  }
  return 'web'
}
