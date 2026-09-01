# SumatraPDF 渲染路径（对照 rustmarkdown PDF 预览）

源码来自 https://github.com/sumatrapdfreader/sumatrapdf （GPLv3），仅作阅读对照，**不链进本程序**。

引擎：Sumatra 用 MuPDF；本程序用 pdfium。光栅速度同一量级，卡顿主要在调度/UI，不在换引擎。

## 本程序原先为什么卡

1. 每帧对 **全部页** 建 Frame / Image / interact（100 页 PDF 每帧 100 套 widget）。
2. 任意页仍是 `Empty` 就 `request_repaint`，多页文档会空转 60fps。
3. 缩放时把槽位改成 Loading，旧图扔掉，屏幕闪「第 n 页…」。
4. 工作线程 FIFO，滚轮/缩放产生的过时 `Render` 仍会画完。
5. BGRA→RGBA 按字节双层循环；每页再拷进 `ColorImage`。

## Sumatra 对应做法（`RenderCache.*` / `DisplayModel.h`）

| Sumatra | 本程序应对 |
| --- | --- |
| 只 Paint 可见页；`FreeNotVisible` | 预计算 page_tops，二分可见区间，只建可见页 widget |
| 拖滚动条 `SB_THUMBTRACK` 立刻改偏移；`pauseRendering`；worker `PageVisibleNearby` 否则丢掉 | 拖条时不上 GPU 纹理、不发新光栅；松手后再渲当前页 |
| 队列最多 8 个请求；同页不同 zoom 则 abort | worker 合并，同页只留最新 width |
| 缺精确 tile 时先 blit 旧图 / 低清 | `Ready` 保留旧 raster，`pending` 表示在重画 |
| 预测渲染最多 4 页、链式、一次一页 | 可见 ±2 页；远页丢纹理 |
| tile 上限约一屏；缓存最多 128 张图 | 按 DIP×zoom×ppp 渲，上限 2400px |
| UI 线程不光栅 | 已在 `pdf-worker` 线程 |

`kMaxPageRequests = 8`，`kMaxBitmapsCached = 128`，`kMaxPredictiveRequests = 4`（见 `RenderCache.h`）。

## 不搬过来的

- 多线程渲同一文档：pdfium 对同一 `FPDF_DOCUMENT` 非线程安全，仍单 worker。
- MuPDF tile / pixmap：继续 pdfium 整页位图（页宽通常 < 一屏；放大再按 width 重渲）。
- GDI `BitBlt`：egui 走 GPU 纹理，缓存 `TextureHandle` 即可。
