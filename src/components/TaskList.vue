<template>
  <el-card class="task-list">
    <template #header>
      <div class="card-header">
        <div class="card-title">
          <span class="title-text">录制任务</span>
          <span class="title-count" v-if="totalRows">{{ totalRows }}</span>
        </div>
        <div class="card-search">
          <el-input
            v-model="searchInput"
            size="small"
            clearable
            placeholder="搜索文件名 / 主播"
            :prefix-icon="Search"
          />
        </div>
        <div class="card-actions" v-if="tasks.length">
          <el-checkbox
            :model-value="allSelected"
            :indeterminate="someSelected"
            @change="toggleAll"
          >全选本页</el-checkbox>
          <el-button
            size="small"
            type="danger"
            plain
            :disabled="selected.size === 0"
            @click="deleteSelected"
          >批量删除{{ selected.size ? ` (${selected.size})` : '' }}</el-button>
          <el-button size="small" @click="clearAll">清空全部</el-button>
        </div>
      </div>
    </template>

    <!-- Empty State -->
    <div v-if="tasks.length === 0" class="empty-state">
      <div class="empty-icon">
        <svg viewBox="0 0 48 48" fill="none">
          <path d="M28 8h-8L16 14H8a4 4 0 00-4 4v18a4 4 0 004 4h32a4 4 0 004-4V18a4 4 0 00-4-4h-8l-4-6z" stroke="currentColor" stroke-width="2" />
          <circle cx="24" cy="28" r="6" stroke="currentColor" stroke-width="2" />
          <path d="M24 24v4l3 2" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" />
        </svg>
      </div>
      <p class="empty-title">{{ search ? '没有匹配的录制记录' : '暂无录制任务' }}</p>
      <p class="empty-desc">{{ search ? '换个关键词试试，或清空搜索' : '在房间列表中点击录制按钮开始' }}</p>
    </div>

    <!-- Task Cards -->
    <div v-else class="task-cards">
      <TransitionGroup name="task">
        <div
          v-for="task in tasks"
          :key="task.rowKey"
          class="task-card"
          :class="task.status"
        >
          <!-- Left accent stripe -->
          <div class="task-accent" :class="task.status"></div>

          <div class="task-main">
            <div class="task-header-row">
              <el-checkbox
                class="task-select"
                :model-value="selected.has(task.rowKey)"
                @change="toggleRow(task)"
              />
              <span class="task-status-badge" :class="task.status">
                <span class="badge-dot"></span>
                {{ getStatusText(task.status, task.conversionState) }}
              </span>
              <span v-if="task.trigger === 'auto'" class="task-trigger-badge">自动</span>
              <span v-if="task.recovery_from_task_id != null" class="task-trigger-badge" :title="`接续任务 ${task.recovery_from_task_id}`">断流恢复</span>
              <span v-if="task.segmentIndex" class="task-trigger-badge">第 {{ task.segmentIndex }} 段<template v-if="task.segmentTotal"> · 共 {{ task.segmentTotal }} 段</template></span>
              <span
                v-if="danmakuCount(task.id) > 0"
                class="task-trigger-badge danmaku-badge"
                :title="`弹幕 ${danmakuCount(task.id)} 条`"
              >弹幕 {{ danmakuCount(task.id) }}</span>
              <span v-else-if="task.danmaku_count && task.danmaku_count > 0" class="task-trigger-badge danmaku-badge">{{ task.danmaku_count }} 弹幕</span>
              <span class="task-time">{{ formatTime(task.start_time) }}</span>
            </div>
            <div class="task-meta">
              <span v-if="task.anchor_name" class="task-anchor">{{ task.anchor_name }}</span>
              <span v-if="task.room_title" class="task-room-title" :title="task.room_title">{{ task.room_title }}</span>
              <span v-if="task.quality" class="task-quality">{{ task.quality }}</span>
              <span v-if="formatDuration(task)" class="task-meta-item">时长 {{ formatDuration(task) }}</span>
              <span class="task-meta-item">{{ formatBytes(task.file_size) }}</span>
              <span v-if="task.file_missing" class="task-missing">文件缺失</span>
            </div>
            <div class="task-path" v-if="task.file_path">
              {{ task.file_path.split(/[/\\]/).pop() }}
            </div>
            <div class="task-path danmaku-path" v-if="task.danmaku_path">
              弹幕：{{ task.danmaku_path.split(/[/\\]/).pop() }}
            </div>
            <div v-if="task.error_message" class="task-error">{{ task.error_message }}</div>
            <div v-if="task.conversionError" class="segment-error">{{ task.conversionError }}</div>
          </div>

          <div class="task-actions">
            <button
              v-if="task.status === 'recording'"
              class="icon-btn warning"
              @click="stopRecord(task)"
              :title="task.segmentId ? '停止本场录制' : '停止录制'"
            >
              <el-icon :size="15"><VideoPause /></el-icon>
            </button>
            <button
              v-if="canAccessFile(task) && isDesktop()"
              class="icon-btn primary"
              @click="openFile(task)"
              title="打开文件"
            >
              <el-icon :size="15"><FolderOpened /></el-icon>
            </button>
            <button
              v-if="canAccessFile(task) && isDesktop()"
              class="icon-btn primary"
              @click="openFolder(task)"
              title="打开文件夹"
            >
              <el-icon :size="15"><Folder /></el-icon>
            </button>
            <el-dropdown
              v-if="task.danmaku_path && task.status !== 'recording'"
              trigger="click"
              @command="(command: string) => onDanmakuAction(task, command)"
            >
              <button class="icon-btn primary" title="弹幕字幕">
                <el-icon :size="15"><ChatDotSquare /></el-icon>
              </button>
              <template #dropdown>
                <el-dropdown-menu>
                  <el-dropdown-item command="ass">导出 ASS 字幕</el-dropdown-item>
                  <el-dropdown-item command="burn">烧录到视频（较慢）</el-dropdown-item>
                </el-dropdown-menu>
              </template>
            </el-dropdown>
            <button
              v-if="canConvert(task)"
              class="icon-btn primary"
              @click="convertToMp4(task)"
              title="转换为 MP4"
            >
              <el-icon :size="15"><Switch /></el-icon>
            </button>
            <button
              class="icon-btn danger"
              @click="deleteTask(task)"
              :disabled="task.status === 'recording' || task.status === 'finalizing'"
              title="删除记录（保留文件）"
            >
              <el-icon :size="15"><Delete /></el-icon>
            </button>
          </div>
        </div>
      </TransitionGroup>
      <div v-if="totalRows > taskPageSize" class="task-pagination">
        <el-pagination
          background
          layout="total, sizes, prev, pager, next"
          :total="totalRows"
          :page-size="taskPageSize"
          :current-page="taskPage"
          :page-sizes="[20, 50, 100]"
          @current-change="changePage"
          @size-change="changeSize"
        />
      </div>
    </div>
  </el-card>
</template>

<script setup lang="ts">
import { storeToRefs } from 'pinia'
import { computed, ref, watch } from 'vue'
import { useRecorderStore } from '../stores/recorder'
import { ElMessage, ElMessageBox } from 'element-plus'
import { Delete, VideoPause, FolderOpened, Folder, Switch, ChatDotSquare, Search } from '@element-plus/icons-vue'
import { isDesktop, openRecordedFile, revealRecordedFile } from '../api/bridge'
import type { DeleteRecordsResult, RecordSegment } from '../types'
import { recordingRows, type TaskRow } from '../utils/recordingRows'

const store = useRecorderStore()
const { tasks: sessions, settings, danmakuCounts, taskPage, taskPageSize, totalRows, search } = storeToRefs(store)

const tasks = computed(() => recordingRows(sessions.value))

const searchInput = ref(search.value)
let searchTimer: ReturnType<typeof setTimeout> | undefined
watch(searchInput, (value) => {
  if (searchTimer) clearTimeout(searchTimer)
  // Debounce so typing does not fire a request per keystroke.
  searchTimer = setTimeout(() => store.setSearch(value), 300)
})
watch(search, (value) => {
  if (value !== searchInput.value) searchInput.value = value
})

const selected = ref<Set<string>>(new Set())
const allSelected = computed(
  () => tasks.value.length > 0 && tasks.value.every(task => selected.value.has(task.rowKey))
)
const someSelected = computed(
  () => !allSelected.value && tasks.value.some(task => selected.value.has(task.rowKey))
)

// A reload replaces the visible rows; stale keys would target invisible records.
watch(tasks, () => {
  selected.value = new Set()
})

function toggleRow(task: TaskRow) {
  const next = new Set(selected.value)
  if (next.has(task.rowKey)) next.delete(task.rowKey)
  else next.add(task.rowKey)
  selected.value = next
}

function toggleAll(value: boolean | string | number) {
  selected.value = value ? new Set(tasks.value.map(task => task.rowKey)) : new Set()
}

async function changePage(page: number) {
  await store.loadTasks(page)
}

async function changeSize(size: number) {
  await store.setTaskPageSize(size)
}

function deleteResultText(result: DeleteRecordsResult) {
  return result.skipped > 0
    ? `已删除 ${result.deleted} 条，跳过 ${result.skipped} 条（录制中或转换中）`
    : `已删除 ${result.deleted} 条`
}

async function deleteSelected() {
  const rows = tasks.value.filter(task => selected.value.has(task.rowKey))
  if (rows.length === 0) return
  try {
    await ElMessageBox.confirm(
      `删除选中的 ${rows.length} 条记录？视频文件将保留。`,
      '批量删除',
      { type: 'warning', confirmButtonText: '删除', cancelButtonText: '取消' }
    )
  } catch {
    return
  }
  const taskIds = [...new Set(rows.filter(row => !row.segmentId).map(row => row.id))]
  const segmentIds = [...new Set(rows.filter(row => row.segmentId).map(row => row.segmentId as number))]
  try {
    const result = await store.deleteRecords(taskIds, segmentIds)
    ElMessage.success(deleteResultText(result))
  } catch (e) {
    ElMessage.error(`删除失败: ${e}`)
  }
}

async function clearAll() {
  try {
    await ElMessageBox.confirm(
      '清空全部录制记录？视频文件将保留，录制中或转换中的记录会跳过。',
      '清空全部',
      { type: 'warning', confirmButtonText: '清空', cancelButtonText: '取消' }
    )
  } catch {
    return
  }
  try {
    const result = await store.clearRecords()
    ElMessage.success(deleteResultText(result))
  } catch (e) {
    ElMessage.error(`清空失败: ${e}`)
  }
}

function danmakuCount(taskId: number) {
  return danmakuCounts.value[taskId] ?? 0
}

function getStatusText(status: string, conversionState?: RecordSegment['conversion_state']) {
  if (conversionState === 'queued') return '等待转换'
  if (conversionState === 'converting') return '转换中'
  if (conversionState === 'failed') return '转换失败 · FLV 已保留'
  const map: Record<string, string> = {
    waiting: '等待中',
    recording: '录制中',
    finalizing: '结束处理中',
    completed: '已完成',
    interrupted: '录制中断',
    failed: '失败',
  }
  return map[status] || status
}

function parseTime(time: string | null): number | null {
  if (!time) return null
  const date = new Date(time.includes('Z') || time.includes('+') ? time : time + 'Z')
  return isNaN(date.getTime()) ? null : date.getTime()
}

function formatDuration(task: TaskRow): string {
  const start = parseTime(task.start_time)
  if (start == null) return ''
  const end = parseTime(task.end_time) ?? (task.status === 'recording' ? Date.now() : null)
  if (end == null) return ''
  const seconds = Math.max(0, Math.floor((end - start) / 1000))
  const h = Math.floor(seconds / 3600)
  const m = Math.floor((seconds % 3600) / 60)
  const s = seconds % 60
  if (h > 0) return `${h}小时${m}分`
  if (m > 0) return `${m}分${s}秒`
  return `${s}秒`
}

function formatBytes(bytes: number | null): string {
  if (!bytes || bytes <= 0) return '—'
  const units = ['B', 'KB', 'MB', 'GB', 'TB']
  let value = bytes
  let index = 0
  while (value >= 1024 && index < units.length - 1) {
    value /= 1024
    index++
  }
  return `${value.toFixed(index === 0 || value >= 10 ? 0 : 1)} ${units[index]}`
}

function formatTime(time: string) {
  if (!time) return ''
  try {
    // SQLite datetime('now') is UTC; append 'Z' to parse correctly
    const date = new Date(time.includes('Z') || time.includes('+') ? time : time + 'Z')
    if (isNaN(date.getTime())) return time

    const now = new Date()

    // Convert to Beijing time (UTC+8) for display
    const bjOffset = 8 * 60 * 60 * 1000
    const bjDate = new Date(date.getTime() + bjOffset)
    const bjNow = new Date(now.getTime() + bjOffset)

    const y = bjDate.getUTCFullYear()
    const mo = bjDate.getUTCMonth()
    const d = bjDate.getUTCDate()
    const h = bjDate.getUTCHours()
    const min = bjDate.getUTCMinutes()

    const ny = bjNow.getUTCFullYear()
    const nmo = bjNow.getUTCMonth()
    const nd = bjNow.getUTCDate()

    const is24h = settings.value.time_format_24h
    const mode = settings.value.time_display_mode

    // Format time portion
    function fmtTime(h: number, m: number): string {
      if (is24h) {
        return `${String(h).padStart(2, '0')}:${String(m).padStart(2, '0')}`
      }
      const period = h < 12 ? 'AM' : 'PM'
      const h12 = h === 0 ? 12 : h > 12 ? h - 12 : h
      return `${h12}:${String(m).padStart(2, '0')} ${period}`
    }

    const timeStr = fmtTime(h, min)

    // Relative mode
    if (mode === 'relative') {
      const diffMs = now.getTime() - date.getTime()
      if (diffMs < 0) return timeStr

      const diffSec = Math.floor(diffMs / 1000)
      const diffMin = Math.floor(diffSec / 60)
      const diffHour = Math.floor(diffMin / 60)
      const diffDay = Math.floor(diffHour / 24)

      if (diffSec < 60) return '刚刚'
      if (diffMin < 60) return `${diffMin} 分钟前`
      if (diffHour < 24) return `${diffHour} 小时前`
      if (diffDay < 30) return `${diffDay} 天前`
      if (diffDay < 365) return `${Math.floor(diffDay / 30)} 个月前`
      return `${Math.floor(diffDay / 365)} 年前`
    }

    // Absolute mode with smart formatting
    if (y !== ny) {
      // Different year: show year/month/day time
      return `${y}/${String(mo + 1).padStart(2, '0')}/${String(d).padStart(2, '0')} ${timeStr}`
    }
    if (mo !== nmo) {
      // Different month: show month/day time
      return `${mo + 1}/${String(d).padStart(2, '0')} ${timeStr}`
    }
    if (d !== nd) {
      // Different day: show day time
      return `${d}日 ${timeStr}`
    }
    // Same day: show time only
    return timeStr
  } catch {
    return time
  }
}

async function stopRecord(task: TaskRow) {
  try {
    const updated = await store.stopRecord(task.id)
    if (updated.status === 'completed') ElMessage.success('录制已停止')
  } catch (e) {
    ElMessage.error(`停止失败: ${e}`)
  }
}

async function onDanmakuAction(task: TaskRow, action: string) {
  const args = { taskId: task.id, segmentId: task.segmentId }
  try {
    if (action === 'ass') {
      const path = await store.exportDanmakuAss(args)
      ElMessage.success(`已生成字幕：${path.split(/[/\\]/).pop()}`)
    } else {
      const loading = ElMessage({ message: '正在烧录弹幕字幕，请稍候…', duration: 0 })
      try {
        const path = await store.burnDanmaku(args)
        ElMessage.success(`烧录完成：${path.split(/[/\\]/).pop()}`)
      } finally {
        loading.close()
      }
    }
  } catch (e) {
    ElMessage.error(`弹幕字幕处理失败: ${e}`)
  }
}

async function openFile(task: TaskRow) {
  if (!task.file_path) {
    ElMessage.warning('文件路径为空')
    return
  }
  try {
    await openRecordedFile({ path: task.file_path })
  } catch (e) {
    ElMessage.error(`打开文件失败: ${e}`)
  }
}

async function openFolder(task: TaskRow) {
  if (!task.file_path) {
    ElMessage.warning('文件路径为空')
    return
  }
  try {
    await revealRecordedFile({ path: task.file_path })
  } catch (e) {
    ElMessage.warning(String(e))
  }
}

function isFlvFile(task: TaskRow): boolean {
  return !!task.file_path && task.file_path.endsWith('.flv')
}

function canAccessFile(task: TaskRow): boolean {
  return !!task.file_path && ['completed', 'interrupted', 'failed'].includes(task.status)
}

function canConvert(task: TaskRow): boolean {
  return canAccessFile(task) && isFlvFile(task)
}

async function convertToMp4(task: TaskRow) {
  try {
    if (task.segmentId) await store.convertSegmentToMp4(task.segmentId)
    else await store.convertToMp4(task.id)
    ElMessage.success('已转换为 MP4')
  } catch (e) {
    ElMessage.error(`转换失败: ${e}`)
  }
}

async function deleteTask(task: TaskRow) {
  try {
    if (task.segmentId) await store.deleteSegment(task.segmentId)
    else await store.deleteTask(task.id)
    ElMessage.success('删除成功')
  } catch (e) {
    ElMessage.error(`删除失败: ${e}`)
  }
}
</script>

<style scoped>
.segment-error {
  margin-top: 4px;
  color: var(--color-warning);
  font-size: 12px;
  overflow-wrap: anywhere;
}
.card-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 12px;
}

.card-title {
  display: flex;
  align-items: center;
  gap: 8px;
}

.title-text {
  font-size: 15px;
  font-weight: 600;
  color: var(--color-text);
  letter-spacing: -0.2px;
}

.title-count {
  font-size: 11px;
  font-weight: 600;
  color: var(--color-text-tertiary);
  background: var(--color-bg);
  min-width: 22px;
  height: 22px;
  padding: 0 6px;
  border-radius: var(--radius-full);
  display: inline-flex;
  align-items: center;
  justify-content: center;
}

/* ── Empty State ── */
.empty-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: 40px 24px 48px;
}

.empty-icon {
  width: 56px;
  height: 56px;
  color: var(--color-border);
  margin-bottom: 16px;
}

.empty-icon svg {
  width: 100%;
  height: 100%;
}

.empty-title {
  font-size: 15px;
  font-weight: 600;
  color: var(--color-text-secondary);
  margin-bottom: 4px;
}

.empty-desc {
  font-size: 13px;
  color: var(--color-text-tertiary);
}

/* ── Task Cards ── */
.task-cards {
  padding: 4px 0;
}

.task-card {
  display: flex;
  align-items: center;
  gap: 14px;
  padding: 12px 20px 12px 0;
  transition: background var(--transition-fast);
  position: relative;
}

.task-card:hover {
  background: var(--color-surface-hover);
}

/* ── Left Accent Stripe ── */
.task-accent {
  width: 3px;
  align-self: stretch;
  border-radius: 0 2px 2px 0;
  flex-shrink: 0;
  opacity: 0;
  transition: opacity var(--transition-fast);
}

.task-card.recording .task-accent {
  background: var(--color-warning);
  opacity: 1;
}

.task-card.completed .task-accent {
  background: var(--color-success);
  opacity: 0.5;
}

.task-card.finalizing .task-accent {
  background: var(--color-primary);
  opacity: 0.5;
}

.task-card.interrupted .task-accent {
  background: var(--color-warning);
  opacity: 0.7;
}

.task-card.failed .task-accent {
  background: var(--color-danger);
  opacity: 0.6;
}

/* ── Task Main ── */
.task-main {
  flex: 1;
  min-width: 0;
  padding-left: 20px;
}

.task-header-row {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-bottom: 3px;
}

.task-status-badge {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  font-size: 11px;
  font-weight: 600;
  padding: 2px 8px;
  border-radius: var(--radius-full);
  background: var(--color-bg);
  color: var(--color-text-tertiary);
  letter-spacing: 0.2px;
}

.task-status-badge.recording {
  background: var(--color-warning-light);
  color: var(--color-warning);
}

.task-status-badge.completed {
  background: var(--color-success-light);
  color: var(--color-success);
}

.task-status-badge.finalizing {
  background: var(--color-primary-light);
  color: var(--color-primary);
}

.task-status-badge.interrupted {
  background: var(--color-warning-light);
  color: var(--color-warning);
}

.task-status-badge.failed {
  background: var(--color-danger-light);
  color: var(--color-danger);
}

.task-status-badge.waiting {
  background: var(--color-primary-light);
  color: var(--color-primary);
}

.badge-dot {
  width: 5px;
  height: 5px;
  border-radius: var(--radius-full);
  background: currentColor;
}

.task-status-badge.recording .badge-dot {
  animation: pulse 1.5s ease-in-out infinite;
}

@keyframes pulse {
  0%, 100% { opacity: 1; transform: scale(1); }
  50% { opacity: 0.4; transform: scale(0.85); }
}

.task-time {
  font-size: 11px;
  color: var(--color-text-tertiary);
  font-variant-numeric: tabular-nums;
}

.task-trigger-badge {
  font-size: 10px;
  font-weight: 600;
  padding: 2px 6px;
  border-radius: var(--radius-full);
  color: var(--color-primary);
  background: var(--color-primary-light);
}

.danmaku-badge {
  color: #e6a23c;
  background: rgba(230, 162, 60, 0.12);
}

.task-path {
  font-size: 12px;
  color: var(--color-text-secondary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  font-family: 'SF Mono', 'Menlo', 'Consolas', monospace;
  font-size: 11px;
}

.card-search {
  width: 220px;
  margin-left: auto;
}

.task-meta {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px;
  margin: 4px 0;
  font-size: 11px;
  color: var(--color-text-tertiary);
}

.task-anchor {
  color: var(--color-text-secondary);
  font-weight: 600;
}

.task-room-title {
  max-width: 220px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.task-quality {
  padding: 1px 6px;
  border-radius: var(--radius-full);
  background: var(--color-primary-light);
  color: var(--color-primary);
  font-weight: 600;
}

.task-missing {
  padding: 1px 6px;
  border-radius: var(--radius-full);
  background: var(--color-danger-light);
  color: var(--color-danger);
  font-weight: 600;
}

.task-error {
  margin-top: 4px;
  color: var(--color-danger);
  font-size: 12px;
  overflow-wrap: anywhere;
}

.danmaku-path {
  color: #b88230;
}

.card-actions {
  display: flex;
  align-items: center;
  gap: 8px;
}

.task-select {
  flex-shrink: 0;
}

.task-pagination {
  display: flex;
  justify-content: center;
  padding: 12px 0 4px;
}

/* ── Actions ── */
.task-actions {
  display: flex;
  align-items: center;
  gap: 2px;
  flex-shrink: 0;
}

.icon-btn {
  width: 30px;
  height: 30px;
  border: none;
  background: transparent;
  border-radius: var(--radius-sm);
  cursor: pointer;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  color: var(--color-text-tertiary);
  transition: all var(--transition-fast);
}

.icon-btn:hover {
  background: var(--color-bg);
  color: var(--color-text-secondary);
}

.icon-btn.primary:hover {
  background: var(--color-primary-light);
  color: var(--color-primary);
}

.icon-btn.warning:hover {
  background: var(--color-warning-light);
  color: var(--color-warning);
}

.icon-btn.danger:hover {
  background: var(--color-danger-light);
  color: var(--color-danger);
}

.icon-btn:disabled {
  opacity: 0.35;
  cursor: not-allowed;
  pointer-events: none;
}

/* ── Transition ── */
.task-enter-active {
  transition: all 0.3s ease;
}

.task-leave-active {
  transition: all 0.2s ease;
}

.task-enter-from {
  opacity: 0;
  transform: translateY(-8px);
}

.task-leave-to {
  opacity: 0;
  transform: translateX(-10px);
}
</style>
