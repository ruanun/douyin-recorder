// Headless reproduction of the settings-save flow against the deployed server.
// Usage: node scripts/repro-settings-save.mjs [baseUrl]
import puppeteer from 'puppeteer-core'

const base = process.argv[2] || 'http://127.0.0.1:8787'
const token = process.env.DOUYIN_RECORDER_TOKEN || ''

const browser = await puppeteer.launch({
  executablePath: '/usr/bin/google-chrome',
  headless: 'new',
  args: ['--no-sandbox', '--disable-gpu', '--disable-dev-shm-usage'],
})
const page = await browser.newPage()
const logs = []
page.on('console', m => logs.push(`[console:${m.type()}] ${m.text()}`))
page.on('pageerror', e => logs.push(`[pageerror] ${e.message}`))
page.on('requestfailed', r => logs.push(`[requestfailed] ${r.url()} ${r.failure()?.errorText}`))
page.on('request', r => { if (r.url().includes('/api/invoke/')) logs.push(`[request] ${r.method()} ${r.url()}`) })
page.on('response', r => {
  if (r.url().includes('/api/')) logs.push(`[response] ${r.status()} ${r.request().method()} ${r.url()}`)
})

const url = token ? `${base}/?token=${encodeURIComponent(token)}` : `${base}/`
console.log('open:', url.replace(token, token ? '<token>' : ''))
await page.goto(url, { waitUntil: 'domcontentloaded', timeout: 30000 })
await page.waitForSelector('.settings-btn', { timeout: 15000 })
console.log('app loaded')
// Dismiss the token prompt if the page was opened without a token.
const prompted = await page.evaluate(() => !!document.querySelector('.el-message-box'))
if (prompted) {
  console.log('token prompt is open (would block interactions without user input)')
  if (token) {
    await page.type('.el-message-box input', token)
    await page.click('.el-message-box .el-button--primary')
    await page.waitForNavigation({ waitUntil: 'domcontentloaded', timeout: 20000 }).catch(() => {})
    await page.waitForSelector('.settings-btn', { timeout: 15000 })
    console.log('token submitted, reloaded')
  }
}

await page.click('.settings-btn')
await page.waitForSelector('.el-dialog', { visible: true, timeout: 10000 })
await page.waitForSelector('.el-dialog .el-overlay-dialog', { timeout: 5000 }).catch(() => {})
console.log('settings dialog open')

// Click the primary "保存" button inside the dialog footer.
const saveClicked = await page.evaluate(() => {
  const buttons = [...document.querySelectorAll('.el-dialog .el-button')]
  const save = buttons.find(b => b.textContent?.trim() === '保存')
  if (!save) return false
  save.click()
  return true
})
console.log('save clicked:', saveClicked)

// Capture any validation message immediately after the click.
for (let i = 0; i < 40; i++) {
  const snapshot = await page.evaluate(() => {
    const messages = [...document.querySelectorAll('.el-message, .el-message__content')].map(m => m.textContent?.trim())
    const loading = !!document.querySelector('.el-dialog .el-button.is-loading')
    return { messages: [...new Set(messages)], loading }
  })
  if (snapshot.messages.length || snapshot.loading) {
    console.log('immediate:', JSON.stringify(snapshot))
    break
  }
  await new Promise(r => setTimeout(r, 50))
}
const formSnapshot = await page.evaluate(() => {
  const inputs = [...document.querySelectorAll('.el-dialog input.el-input__inner')]
  return inputs.map(i => ({ value: i.value, type: i.type }))
})
console.log('form inputs:', JSON.stringify(formSnapshot.slice(0, 12)))
// Watch for the dialog closing (success) or an error message appearing.
const started = Date.now()
let result = 'timeout'
while (Date.now() - started < 15000) {
  const state = await page.evaluate(() => {
    const dialogVisible = !!document.querySelector('.el-dialog')
      && getComputedStyle(document.querySelector('.el-dialog')?.closest('.el-overlay') || document.body).display !== 'none'
    const messages = [...document.querySelectorAll('.el-message')].map(m => m.textContent?.trim())
    const loading = !!document.querySelector('.el-dialog .el-button.is-loading')
    return { dialogVisible, messages, loading }
  })
  if (state.messages.length) {
    result = `message: ${state.messages.join(' | ')}`
    break
  }
  if (!state.dialogVisible) {
    result = 'dialog closed (saved)'
    break
  }
  await new Promise(r => setTimeout(r, 250))
}
console.log('result:', result, `after ${Date.now() - started}ms`)
console.log('--- browser log ---')
for (const line of logs.slice(-40)) console.log(line)
await browser.close()
