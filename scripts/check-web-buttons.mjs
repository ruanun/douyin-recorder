import puppeteer from 'puppeteer-core'
const base = 'http://127.0.0.1:8787'
const token = process.env.DOUYIN_RECORDER_TOKEN
const browser = await puppeteer.launch({
  executablePath: '/usr/bin/google-chrome', headless: 'new',
  args: ['--no-sandbox', '--disable-gpu', '--disable-dev-shm-usage'],
})
const page = await browser.newPage()
await page.goto(`${base}/?token=${encodeURIComponent(token)}`, { waitUntil: 'domcontentloaded' })
await page.waitForSelector('.task-card, .empty-state', { timeout: 20000 })
await new Promise(r => setTimeout(r, 1500))
const buttons = await page.$$eval('.task-card button', els => els.map(el => el.getAttribute('title')).filter(Boolean))
console.log('任务卡片按钮:', JSON.stringify([...new Set(buttons)]))
console.log('包含打开文件:', buttons.includes('打开文件'))
console.log('包含打开文件夹:', buttons.includes('打开文件夹'))
console.log('包含弹幕字幕:', buttons.includes('弹幕字幕'))
await browser.close()
