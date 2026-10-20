// Headless regression test for the "danmaku subtitle tool" upload flow.
// Usage: node scripts/repro-danmaku-upload.mjs [baseUrl]
import { writeFileSync } from 'node:fs'
import puppeteer from 'puppeteer-core'

const base = process.argv[2] || 'http://127.0.0.1:8787'
const token = process.env.DOUYIN_RECORDER_TOKEN || ''

const sample = `<?xml version="1.0" encoding="utf-8"?>
<i>
<d p="1.500,1,25,16777215,1760341234567,0,42,0" user="观众甲">哈哈哈</d>
<d p="2.000,5,25,16711680,1760341234999,0,43,0" user="观众乙">顶部</d>
<d p="3.250,4,25,255,1760341235555,0,44,0" user="观众丙">底部</d>
</i>`
const samplePath = '/tmp/douyin-upload-sample.xml'
writeFileSync(samplePath, sample)

const browser = await puppeteer.launch({
  executablePath: '/usr/bin/google-chrome',
  headless: 'new',
  args: ['--no-sandbox', '--disable-gpu', '--disable-dev-shm-usage'],
})
const page = await browser.newPage()
const logs = []
page.on('console', m => logs.push(`[console:${m.type()}] ${m.text()}`))
page.on('pageerror', e => logs.push(`[pageerror] ${e.message}`))

const url = token ? `${base}/?token=${encodeURIComponent(token)}` : `${base}/`
await page.goto(url, { waitUntil: 'domcontentloaded', timeout: 30000 })
await page.waitForSelector('.danmaku-tool-btn', { timeout: 15000 })
await page.click('.danmaku-tool-btn')
await page.waitForSelector('.el-dialog', { visible: true, timeout: 10000 })
console.log('dialog opened')

const input = await page.$('.el-dialog input[type=file]')
if (!input) throw new Error('file input not found')
await input.uploadFile(samplePath)
await page.waitForFunction(
  () => !!document.querySelector('.file-row .file-name'),
  { timeout: 10000 },
)
console.log('file selected:', await page.$eval('.file-row .file-name', el => el.textContent))

const clicked = await page.evaluate(() => {
  const buttons = [...document.querySelectorAll('.el-dialog .el-button')]
  const target = buttons.find(b => b.textContent?.trim() === '转换为 ASS')
  if (!target) return false
  target.click()
  return true
})
console.log('convert clicked:', clicked)

const started = Date.now()
let outcome = 'timeout'
while (Date.now() - started < 15000) {
  const state = await page.evaluate(() => ({
    results: [...document.querySelectorAll('.result-row')].map(el => el.textContent?.trim()),
    error: document.querySelector('.error-row')?.textContent?.trim() || '',
    messages: [...document.querySelectorAll('.el-message')].map(el => el.textContent?.trim()),
  }))
  if (state.error) { outcome = `error: ${state.error}`; break }
  if (state.results.some(text => text?.includes('已保存到服务器') || text?.includes('已保存'))) {
    outcome = `ok: ${state.results.join(' | ')} | ${state.messages.join(' | ')}`
    break
  }
  await new Promise(r => setTimeout(r, 200))
}
console.log('outcome:', outcome, `(${Date.now() - started}ms)`)
for (const line of logs.slice(-10)) console.log(line)
await browser.close()
