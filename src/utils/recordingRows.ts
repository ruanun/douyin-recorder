import type { RecordTask, RecordSegment } from '../types'

export interface TaskRow {
  id: number
  rowKey: string
  segmentId?: number
  segmentIndex?: number
  segmentTotal: number
  status: RecordTask['status']
  start_time: string
  end_time: string | null
  file_path: string | null
  file_size: number | null
  file_missing: boolean
  trigger: RecordTask['trigger']
  recovery_from_task_id: number | null
  anchor_name: string
  room_title: string
  quality: string
  error_message: string | null
  danmaku_path: string | null
  danmaku_count: number | null
  conversionState?: RecordSegment['conversion_state']
  conversionError?: string | null
}

export function recordingRows(sessions: RecordTask[]): TaskRow[] {
  return sessions.flatMap<TaskRow>(task => {
    if (!task.segment_output || !task.segments?.length) {
      return [{ ...task, rowKey: `task-${task.id}`, segmentTotal: 0 }]
    }
    return task.segments.filter(segment => !segment.deleted).map(segment => ({
      id: task.id,
      rowKey: `segment-${segment.id}`,
      segmentId: segment.id,
      segmentIndex: segment.segment_index,
      segmentTotal: task.segment_total,
      status: (['queued', 'converting'].includes(segment.conversion_state) ||
        (task.status === 'finalizing' && segment.status === 'recording')) ? 'finalizing' as const : segment.status,
      start_time: segment.start_time,
      end_time: segment.end_time,
      file_path: segment.file_path,
      file_size: segment.file_size,
      file_missing: segment.file_missing,
      trigger: task.trigger,
      recovery_from_task_id: task.recovery_from_task_id,
      anchor_name: task.anchor_name,
      room_title: task.room_title,
      quality: task.quality,
      error_message: task.error_message,
      danmaku_path: segment.danmaku_path ?? task.danmaku_path,
      danmaku_count: segment.danmaku_count ?? task.danmaku_count,
      conversionState: segment.conversion_state,
      conversionError: segment.conversion_error,
    }))
  }).sort((a, b) => b.start_time.localeCompare(a.start_time) || b.id - a.id || (b.segmentIndex ?? 0) - (a.segmentIndex ?? 0))
}
