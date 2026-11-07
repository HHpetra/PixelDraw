# PixelDraw

[English](README.md) | [简体中文](README.zh-CN.md)

让 AI 通过明确的坐标和受约束的 MARD 色表绘制像素画的本地 MCP 工具，而不是直接调用文生图模型。

Rust 程序负责执行绘图指令，MCP 客户端负责提供模型。PixelDraw 本身不需要 API Key，也不开放网络监听端口；模型服务的配置和费用由客户端负责。

## 功能介绍

在 MCP 客户端中描述想画的内容，AI 就可以创建画布，用直线/矩形/三角形/圆/椭圆/像素菱形/曲线/油漆桶/整张背景填充或像素落点画出图形；画完可以出图看看效果，再继续修改和调整（画错可撤回），满意后保存。

- **精确像素绘制：** 画布宽高均支持 1 至 128 像素，通过坐标控制每个落点。
- **固定色表：** 支持 24、144、221 色 MARD 色表，可以使用色号或当前色表的中文颜色名。
- **多尺寸笔刷：** 支持 1×1、2×2、4×4、8×8 方形笔刷，也可用作直线/曲线/描边线宽。
- **曲线绘制：** `draw_curve` 支持二次/三次贝塞尔，或过多个路径点的平滑样条。
- **撤回：** `undo` 可撤回最近的绘制步骤；`draw_batch` 整批记一步。
- **整批校验：** 格式、颜色、坐标或笔刷不合法时，整批指令被拒绝，不会只画上一部分。
- **即时预览：** 创建和绘制后返回最近邻放大 8 倍的 PNG，在支持图片工具结果的客户端中查看。
- **本地保存：** 可以指定输出目录；自动命名遇到重名会重试，显式同名报错，不覆盖已有作品。

适合制作小图标、像素插画，以及探索 AI 通过工具调用进行绘图的过程。它不是独立的 AI 模型、照片转像素画工具或完整图像编辑器。

## 效果展示

以下选用项目 `output/` 中已有的作品，复制到 `docs/images/`，供 GitHub 展示。导出的 PNG 为原始画布的 8 倍大小；示例不代表模型每次都会生成相同的作品。

| 可颂 | 海边日落 | 天坛 |
| --- | --- | --- |
| ![可颂像素画](docs/images/croissant-32x32.png) | ![海边日落像素画](docs/images/seaside-sunset-32x32.png) | ![天坛像素画](docs/images/temple-of-heaven-64x64.png) |
| 32×32 画布 | 32×32 画布 | 64×64 画布 |

## 安装

### 免编译安装（推荐）

打开 [最新 Release](https://github.com/HHpetra/PixelDraw/releases/latest)，展开 **Assets**，下载适合系统的安装包。不要选择 GitHub 自动生成的 **Source code** 源码压缩包。

| 系统 | 文件名后缀 |
| --- | --- |
| Windows x64 | `x86_64-pc-windows-msvc.zip` |
| Linux x64（glibc 2.35 及以上，例如 Ubuntu 22.04 及以上） | `x86_64-unknown-linux-gnu.tar.gz` |
| macOS Apple Silicon（ARM64，M 系列芯片） | `aarch64-apple-darwin.tar.gz` |
| macOS Intel（x64） | `x86_64-apple-darwin.tar.gz` |

解压到固定目录。Windows 程序为 `pixeldraw.exe`，Linux/macOS 为 `pixeldraw`，包内附使用文档。运行服务不需要安装 Rust、Cargo、Git 或额外的模型运行环境；AI 模型仍由 MCP 客户端提供。

Windows PowerShell 示例（路径按实际解压位置修改）：

```powershell
& "C:/Tools/PixelDraw/pixeldraw.exe" --version
& "C:/Tools/PixelDraw/pixeldraw.exe" --help
```

Linux/macOS 解压与检查示例（这里以 Apple Silicon 安装包为例）：

```sh
tar -xzf pixeldraw-v0.2.2-aarch64-apple-darwin.tar.gz
./pixeldraw-v0.2.2-aarch64-apple-darwin/pixeldraw --version
```

然后在 OpenCode 中配置程序的**绝对路径**，不再使用 `cargo run`：

```json
{
  "mcp": {
    "pixeldraw": {
      "type": "local",
      "command": ["C:/Tools/PixelDraw/pixeldraw.exe", "--output-dir", "C:/Pictures/PixelDraw"],
      "enabled": true
    }
  }
}
```

Linux/macOS 将程序路径改为实际位置，例如 `/home/you/tools/pixeldraw` 或 `/Users/you/tools/pixeldraw`，同时替换输出目录。配置完成后重启或重新加载 MCP 客户端，由客户端启动服务。双击程序不会出现绘图窗口，它是供 AI 调用的后台工具。

每个压缩包附有 `.sha256` 校验文件。PowerShell 使用 `Get-FileHash <压缩包> -Algorithm SHA256` 对照校验值；Linux 使用 `sha256sum -c <压缩包>.sha256`；macOS 使用 `shasum -a 256 -c <压缩包>.sha256`。压缩包与校验文件应放在同一目录。校验值用于验证文件完整性，不等于发布者身份签名。

程序尚未进行代码签名或 macOS 公证，系统可能显示安全提示。确认来自本仓库 Release 后，按系统提供的单文件批准流程处理，不要关闭系统级安全防护。

### 源码安装（开发者）

通过 [rustup](https://rustup.rs/) 安装 Rust 1.96.1 或更新版本，然后执行：

```sh
git clone https://github.com/HHpetra/PixelDraw.git
cd PixelDraw
cargo install --path . --locked
pixeldraw --help
```

程序安装到 Cargo 的 bin 目录，该目录需要在 PATH 中。从 v0.1.1 开始提供预编译安装包。CI 在 Windows、Linux 和 macOS 上运行，具体结果可查看仓库 Actions 页面。

## 连接 MCP 客户端

配置 stdio MCP 服务，启动命令为 `pixeldraw`，参数为 `--output-dir` 和输出目录。建议使用绝对路径；如果桌面客户端无法找到程序，也需要指定可执行文件的绝对路径。

OpenCode 配置示例：

```json
{
  "$schema": "https://opencode.ai/config.json",
  "mcp": {
    "pixeldraw": {
      "type": "local",
      "command": ["pixeldraw", "--output-dir", "/absolute/path/to/art"],
      "enabled": true
    }
  }
}
```

将示例目录替换为实际目录。Windows 可以使用 `C:/Users/you/Pictures/PixelDraw` 这样的路径。仓库中的 `opencode.json` 是从项目目录启动 Cargo 的开发配置。

自动化测试覆盖 MCP 通信流程，但不同客户端版本的配置方式和图片显示能力可能不同。

## 绘制第一张图

可以向 AI 提出：

> 请使用MCP工具绘制一幅像素画，内容为海边日落。尺寸为32*32

图元与精确描点都可用。画完可查看预览，再继续修改和调整。最小流程如下，花括号内为工具参数，而不是终端命令：

```text
create_canvas {"width":16,"height":16,"palette":"24"}
draw_circle   {"x":2,"y":2,"diameter":12,"color":"C4","fill":true}
draw_rect     {"x0":2,"y0":2,"x1":13,"y1":13,"color":"A4","fill":false,"brush":2}
save_image    {"filename":"first-icon.png"}
```

这会画一个填充圆和一圈描边，输出一张 128×128 的 PNG。工具返回文字和 PNG 图片内容，需要支持图片工具结果的客户端才能直接预览。

## 选择导出类型

`save_image.output` 有三个选项，省略时仍只输出像素画，兼容已有调用：

| output | 保存内容 | filename 为 `cat.png` 时 |
| --- | --- | --- |
| `pixel`（默认） | 8 倍像素画 PNG；空格透明 | `cat.png` |
| `pattern` | 拼豆图纸 PNG | `cat.png` |
| `both` | 同一快照的像素画和图纸，按这个顺序返回两张图片 | `cat.png`、`cat-pattern.png` |

图纸四边是浅蓝色序号带，行列都从 1 计。每格写 MARD 色号；空格留白、不写色号。黑色计数线从画面中心向外每 10 格一条，并画出外框。页眉写尺寸、色板、总颜色数和总拼豆数，右上角为小红书标志和豆图工坊。下方方色块分两行写色号和数量（例如 `H2` 与 `(12)`），不加颜色名称。导出边长不超过 2048 像素（小红书超过会二次压缩导致色号变糊），色号与序号加大加粗并做描边加粗，缩小后仍可读。图纸使用内置字体，不依赖 Python、系统字体或联网；当前不提供 SVG/PDF 或自动拆页，也不是实物 1:1 定位纸。画布坐标仍从 0 开始。

空格与白色豆严格区分：

```text
create_canvas {"width":24,"height":24,"palette":"221","background":"empty"}
draw_pixels   {"pixels":"0 0 H2 1 0 H7 2 0 F5"}
draw_pixels   {"pixels":"2 0 -"}
save_image    {"filename":"cat.png","output":"both"}
```

`background` 默认为 `white`（整张铺 H2 白豆）；设成 `empty` 时不放豆。可选 `background_color`（中文名或 MARD 色号）会整张铺该色豆，并记为当前背景色；省略时仍按 `white` / `empty`。`background` 为 `empty` 时不能同时传 `background_color`。`draw_pixels` 的 `-` 表示清除，支持相同的笔刷与整批校验。H2 始终是需要放的白色豆，空格不标号、不计数；上例只需 2 颗豆。白色区域不会自动当作背景删除。

两种导出共用规范化色号和占用信息，不从 RGB 反查色号。重叠绘制只统计最后留下的豆子。

每个文件单独原子保存；`both` 预检两个文件名，如已有文件则不写入。多个文件并非整体事务，磁盘故障或并发写入导致中途失败时，错误会列出本次已经保存的文件。省略文件名时整组重名会按数字后缀重试。`both` 的衍生文件名也必须满足 200 字节限制，超长会在写文件前报错。

## 工具与约束

| 工具 | 功能 |
| --- | --- |
| `create_canvas` | 创建画布（`background`: white 默认白豆 / empty 空格）。可选 `background_color` 用色表颜色铺满并记为当前背景色；省略时保持原来的 white / empty。`empty` 不能与 `background_color` 同时使用。宽高 1 至 128；锁定 24、144 或默认的 221 色表。成功创建会替换当前画布；颜色非法时不覆盖。 |
| `list_colors` | 查询当前锁定的色表；尚未创建画布时可以指定要查询的色表。仅在需要查色时调用。 |
| `draw_line` | 画直线；`brush` 为线宽 1/2/4/8，坐标不必对齐网格。 |
| `draw_rect` | 矩形；`fill` 区分填充/描边，描边可用 `brush` 加粗。 |
| `draw_triangle` | 三角形；`fill` 区分填充/描边，描边可用 `brush` 加粗。 |
| `draw_circle` | 圆。左上角 `(x, y)` 与直径 `diameter`（≥ 1 的整数，超过 512 按 512），占据 `diameter×diameter` 格；格心落在圆内才计入。锚点必须在画布内，右侧和下方超出裁剪。`fill` 区分填充与 1 格轮廓；描边可用 `brush` 加粗，填充忽略笔刷。 |
| `draw_ellipse` | 椭圆。左上角 `(x, y)` 与宽 `width`、高 `height`（规则与圆相同）。宽高相等时与同直径的圆逐格相同。 |
| `draw_diamond` | 像素菱形。中心格 `(cx, cy)`（不是左上角锚定）与菱形半轴 `rx`、`ry`（从中心向一侧的格数）；宽高为 `2*rx+1` 与 `2*ry+1`，总是奇数。这组 `rx`/`ry` 是菱形半轴，不是已废止的椭圆半轴。格心落在菱形内才计入。半轴超过 255 按 255。中心格必须在画布内，越出边缘的部分裁剪。`fill: false` 取该填充的 1 格轮廓。描边可用 `brush`，填充忽略笔刷。也可作为 `draw_batch` 的 `diamond` 操作。 |
| `draw_curve` | 曲线：`points` 为 `[x,y]` 列表；2 点直线、3 点二次贝塞尔、4 点三次贝塞尔、≥5 点过点平滑样条。 |
| `flood_fill` | 油漆桶，四连通替换种子点同色号区域；空格与 H2 白豆分开处理；`color` 可用 `-` 清为空格。 |
| `fill_background` | 用一种颜色（中文名或 MARD 色号）强行覆盖每一个格子，包括空格和已有画面，并把已记录的背景色改为该颜色。颜色非法时画布和背景色都不变。有格子变化，或背景色不同时，记一次 undo；格子和背景色都已是该颜色时不改动、不记步。undo 会恢复之前的背景色。也可作为 `draw_batch` 的一步。 |
| `move_region` | 移动一块含端点的矩形（反序自动归一化）。用相对位移 `dx`/`dy` 或目标左上角绝对坐标 `tx`/`ty` 寻址，必须且只能给一组。空格随整块搬走。源区域改回当前背景色；背景为空格时留空格。不改变已记录的背景色。源端点或目标区域越界时整次拒绝。内容无变化时不记 undo。也可作为 `draw_batch` 的一步。 |
| `copy_region` | 参数与 `move_region` 相同。源区域保留，空格会覆盖目标原有内容。 |
| `draw_batch` | 多笔有序批量，原子执行；任一步失败整批拒绝，只回最终预览；成功整批只记一次 undo。操作类型包含 `fill_background`、`move_region`、`copy_region` 和 `motif`。 |
| `draw_pixels` | 散点/方块笔刷落点：`x y 颜色` 三元组；`-` 清除为空格。 |
| `draw_motif` | 盖一个预置小图形。左上角 `(x, y)` 锚定占据矩形（放大后即放大矩形）。一次只上一种规范化色号；`-` 清为空格，与 H2 白豆不同。`scale` 为 1–8 的整数（默认 1），每一占用格铺成 `scale×scale` 实心方块；占位里的空位不写。锚点必须在画布内，越出部分裁剪。覆盖已有内容。不改已记录的背景色。一次盖印记一步 undo；没有格子被写到时不记步。图形名不存在、骰子面缺 `face` 或超出 1–6、非骰子面传了 `face`、`scale` 非法、颜色非法或落点越界时整次拒绝，画布不变。也可作为 `draw_batch` 的 `motif` 操作。 |
| `undo` | 撤回绘制步骤（`steps` 可选，默认 1）。每次成功绘制记一步，`draw_batch` 整批记一步；`create_canvas` 清空历史。 |
| `save_image` | `output`: pixel（默认像素画）、pattern（拼豆图纸）、both（两者），均为 PNG，不覆盖文件。 |

- 左上角为 `(0,0)`，向右为 X 正方向，向下为 Y 正方向。
- 多笔请用 `draw_batch`，不要并行调用多个绘制工具。画完看预览，可继续修改和调整。
- 线宽/描边笔刷为 1、2、4 或 8，从落点向右下扩展 `N×N`（与 `draw_pixels` 足迹一致）；填充与油漆桶忽略笔刷。
- `draw_pixels` 的笔刷坐标必须是笔刷尺寸的倍数，整个方块都必须在画布内。
- 重复落点按输入顺序覆盖；非法批次不会修改画布。
- `fill_background` 覆盖每一个格子，并把当前背景色改为新颜色；不会像 `flood_fill` 那样停在同色区域边界，也不接受 `-` 清为空格。`undo` 会一并恢复之前的背景色。
- 小图形的落点 `(x, y)` 必须在画布内。`scale` 为 1 到 8 的整数，默认 1。`face` 只属于「骰子面」，取 1–6。箭头-左按 4×3 锁定为 `.#../####/..#.`：规格书末行 `. . # . .` 多出的一格是空位，不占宽度，六个占用格坐标不变，也不改成箭头-右的左右镜像。32 个图形名（骰子面按 face 展开 6 份点阵）：十字（3×3、5 格）、叉（3×3、5 格）、品字-缺左上（2×2、3 格）、品字-缺右上（2×2、3 格）、斜线-短（2×2、2 格）、斜线-长（3×3、3 格）、四点（3×3、4 格）、T字-下（3×3、5 格）、T字-上（3×3、5 格）、T字-右（3×3、5 格）、T字-左（3×3、5 格）、拐角-左上（2×2、3 格）、拐角-右上（2×2、3 格）、拐角-左下（2×2、3 格）、拐角-右下（2×2、3 格）、箭头-上（3×4、6 格）、箭头-下（3×4、6 格）、箭头-左（4×3、6 格）、箭头-右（4×3、6 格）、对勾（3×3、4 格）、感叹号（1×5、4 格）、四芒闪光（3×3、4 格）、五角星（5×5、13 格）、闪电（3×5、7 格）、小爱心（5×4、11 格）、水滴（3×4、7 格）、小花（5×5、13 格）、骰子面（3×3，face=1 为 1 格、face=2 为 2 格、face=3 为 3 格、face=4 为 4 格、face=5 为 5 格、face=6 为 6 格）、扑克-红心（5×4、11 格，与小爱心同一点阵）、扑克-方块（5×5、13 格）、扑克-梅花（5×5、12 格）、扑克-黑桃（5×5、13 格）。
- 圆和椭圆的锚点 `(x, y)` 必须在画布内。`diameter`、`width`、`height` 为 ≥ 1 的整数，超过 512 时按 512 计算；右侧和下方超出部分自动裁剪。格心落在椭圆内才计入。`fill: false` 画该填充的 1 格轮廓。圆和椭圆上的旧参数 `cx`、`cy`、`radius`、`rx`、`ry` 会报错。像素菱形用中心格 `(cx, cy)` 与菱形半轴 `rx`、`ry`（不是左上角锚定，也不是已废止的椭圆半轴）；宽高为 `2*rx+1` 与 `2*ry+1`，总是奇数，超过 255 按 255。中心格必须在画布内，越出边缘的部分裁剪。端点、曲线路径点、种子点必须在画布内。区域搬运的源端点越界，或目标区域越界，整次拒绝，画布与 undo 都不变。
- 每个服务实例只有一张内存画布；重启会丢失未保存内容。撤销历史仅存内存（最多 64 步），`create_canvas` 会清空。暂不支持导入、会话持久化或原始尺寸导出。
- 像素画预览通过最近邻采样放大 8 倍；拼豆图纸使用独立的网格布局。24、144、221 三套色表中，同一个 MARD 色号使用相同的中文名和 RGB。

## 输出与排错

`--output-dir PATH` 指定保存目录，默认是服务运行目录下的 `./output`，不是源码或可执行文件所在目录。请使用自己控制且可写的目录；本地工具不是文件系统沙箱。

文件名只接受 ASCII 字母、数字、点、下划线和连字符；补全 `.png` 后最多 200 字节。路径、尾点和 Windows 设备名会被拒绝。省略 `filename` 可自动命名并重试冲突；显式文件名已存在时返回错误。

直接在终端启动后看似没有反应是正常现象：服务正在等待标准输入上的 MCP 消息。日志写入标准错误。保存失败时检查目录权限；颜色被拒绝时先调用 `list_colors`。需要诊断日志时可设置环境变量 `RUST_LOG=debug`。

## 开发与许可

开发检查见 [CONTRIBUTING.md](CONTRIBUTING.md)，完整绘制说明与色表见 [AGENTS.md](AGENTS.md)。代码采用 [MIT 许可证](LICENSE)，色表来源及第三方权利范围见 [THIRD_PARTY.md](THIRD_PARTY.md)。

仓库只收录精选展示图片；本地参考照片、截图和其余 `output/` 内容不随文档发布。
