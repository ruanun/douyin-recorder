import puppeteer from 'puppeteer-core'
const browser = await puppeteer.launch({ executablePath: '/usr/bin/google-chrome', headless: 'new', args: ['--no-sandbox','--disable-gpu','--disable-dev-shm-usage'] })
const page = await browser.newPage()
const start = Date.now()
await page.goto('http://127.0.0.1:8799/?token=bench', { waitUntil: 'domcontentloaded', timeout: 60000 })
await page.waitForFunction(() => document.querySelectorAll('.task-card').length > 0, { timeout: 60000 })
const first = await page.evaluate(() => ({
  cards: document.querySelectorAll('.task-card').length,
  nodes: document.querySelectorAll('*').length,
  more: !!document.querySelector('.load-more button'),
  hint: document.querySelector('.load-more-hint')?.textContent?.trim() || '',
}))
console.log(`首屏渲染: ${Date.now()-start}ms  卡片=${first.cards} 节点=${first.nodes} 有加载更多=${first.more} 提示="${first.hint}"`)
const t2 = Date.now()
await page.click('.load-more button')
await page.waitForFunction(n => document.querySelectorAll('.task-card').length > n, { timeout: 60000 }, first.cards)
const second = await page.evaluate(() => document.querySelectorAll('.task-card').length)
console.log(`加载更多: ${Date.now()-t2}ms  卡片=${second}`)
await browser.close()
