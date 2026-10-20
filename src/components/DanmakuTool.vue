<template>
  <el-dialog
    v-model="visible"
    title="弹幕字幕工具"
    width="480"
    :close-on-click-modal="!busy"
    :close-on-press-escape="!busy"
    :show-close="!busy"
  >
    <div class="tool-body">
      <div class="tool-hint">
        选择 XML 弹幕文件（B 站兼容格式）转换为 ASS 字幕，保存到录制目录。
        Web 模式不提供下载，请在服务器的录制目录中取用。
      </div>

      <input
        ref="fileInput"
        type="file"
        accept=".xml,text/xml,application/xml"
        class="hidden-input"
        @change="onFileSelected"
      />
      <el-button type="primary" plain :disabled="busy" @click="pickFile">
        <el-icon><Upload /></el-icon>
        选择 XML 文件
      </el-button>

      <div v-if="fileName" class="file-row">
        <span class="file-name">{{ fileName }}</span>
        <el-button link type="danger" :disabled="busy" @click="reset">清除</el-button>
      </div>

      <div v-if="commentCount != null" class="result-row">
        <el-icon><CircleCheck /></el-icon>
        已解析 {{ commentCount }} 条弹幕
      </div>
      <div v-if="savedPath" class="result-row">
        <el-icon><CircleCheck /></el-icon>
        {{ isDesktop() ? '已保存' : '已保存到服务器' }}：{{ savedPath }}
      </div>
      <div v-if="errorMessage" class="error-row">{{ errorMessage }}</div>
    </div>

    <template #footer>
      <div class="footer-row">
        <span class="footer-hint">仅转换，不上传到服务器</span>
        <div>
          <el-button :disabled="busy" @click="visible = false">关闭</el-button>
          <el-button type="primary" :loading="busy" :disabled="!fileContent" @click="convert">
            转换为 ASS
          </el-button>
        </div>
      </div>
    </template>
  </el-dialog>
</template>

<script setup lang="ts">
import { ref } from 'vue'
import { ElMessage } from 'element-plus'
import { CircleCheck, Upload } from '@element-plus/icons-vue'
import { invokeCommand, isDesktop } from '../api/bridge'

const visible = defineModel<boolean>({ default: false })

const fileInput = ref<HTMLInputElement>()
const fileName = ref('')
const fileContent = ref('')
const commentCount = ref<number | null>(null)
const savedPath = ref('')
const errorMessage = ref('')
const busy = ref(false)

function reset() {
  fileName.value = ''
  fileContent.value = ''
  commentCount.value = null
  savedPath.value = ''
  errorMessage.value = ''
  if (fileInput.value) fileInput.value.value = ''
}

function pickFile() {
  reset()
  fileInput.value?.click()
}

async function onFileSelected(event: Event) {
  const input = event.target as HTMLInputElement
  const file = input.files?.[0]
  if (!file) return
  if (!file.name.toLowerCase().endsWith('.xml')) {
    errorMessage.value = '请选择 .xml 弹幕文件'
    return
  }
  try {
    fileContent.value = await file.text()
    fileName.value = file.name
    errorMessage.value = ''
  } catch (error) {
    errorMessage.value = `读取文件失败: ${error}`
  }
}

async function convert() {
  if (!fileContent.value) return
  busy.value = true
  errorMessage.value = ''
  savedPath.value = ''
  try {
    const result = await invokeCommand<{ ass: string; count: number }>('convert_danmaku_xml', {
      content: fileContent.value,
    })
    commentCount.value = result.count
    if (result.count === 0) {
      errorMessage.value = '文件中没有可转换的弹幕'
      return
    }
    const name = fileName.value.replace(/\.xml$/i, '') || 'danmaku'
    savedPath.value = await invokeCommand<string>('save_danmaku_ass', {
      fileName: name,
      content: result.ass,
    })
    ElMessage.success(`ASS 字幕已保存（${result.count} 条弹幕）`)
  } catch (error) {
    errorMessage.value = `转换失败: ${error}`
  } finally {
    busy.value = false
  }
}

// Exposed for tests/automation.
defineExpose({ pickFile, convert })
</script>

<style scoped>
.tool-body {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.tool-hint {
  font-size: 12px;
  color: var(--color-text-secondary);
  line-height: 1.6;
}

.hidden-input {
  display: none;
}

.file-row {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 13px;
}

.file-name {
  font-family: 'SF Mono', 'Menlo', 'Consolas', monospace;
  overflow-wrap: anywhere;
}

.result-row {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
  color: var(--color-success, #67c23a);
  overflow-wrap: anywhere;
}

.error-row {
  font-size: 12px;
  color: var(--el-color-danger);
  overflow-wrap: anywhere;
}

.footer-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.footer-hint {
  font-size: 11px;
  color: var(--color-text-secondary);
}
</style>
