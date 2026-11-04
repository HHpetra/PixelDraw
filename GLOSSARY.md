# PixelDraw

A local MCP server that lets an AI draw pixel art and bead patterns with explicit pixel coordinates on a constrained MARD palette. These are the project's domain terms; use them in tool names, issues and docs.

## Language

**豆子（bead）**:
画布一格上实际要放的 MARD 色号实体；拼豆图纸按它逐格标注与计数。
_Avoid_: 像素点, 颜色, pixel

**空格（empty cell）**:
没有放豆的格子，没有色号、不参与图纸计数；与放着 H2 白豆的格子是两件事。
_Avoid_: 透明格, 空白, 背景

**像素圆（pixel circle）**:
用格子拼出的圆，尺寸按直径表达、位置按占据方形的左上角锚定；与数学圆不同，它的顶端是一段平台而不是一个点。
_Avoid_: 圆形图元, circle 轮廓, 正圆

**直径（diameter）**:
像素圆占用的格子数，同时也是它占据的方形范围的边长；奇数直径有正中一格，偶数直径没有。
_Avoid_: 半径, radius, 圆宽

**椭圆尺寸（ellipse extent）**:
像素椭圆水平与垂直两个方向各自占用的格子数（宽度、高度），取代半轴表达；宽高相等时就是像素圆。
_Avoid_: 半轴, rx, ry, 轴长

**格心（cell center）**:
一个格子的几何中心，位于格子左上角再加半格；判断一格属不属于像素圆这类图元，只看它的格心在不在图形内。
_Avoid_: 像素中心, 格中点, 格角

**规范化色号（canonical bead code）**:
写入时一次性解析并存储的 MARD 色号（如 `H2`、`F2`），是占用信息与图纸的唯一真源，不从 RGB 反查。
_Avoid_: 颜色索引, color id, 调色板下标

**背景色（background color）**:
画布记住的底色色号，由 `create_canvas` 或 `fill_background` 写入，绘制工具不改动它；`background: empty` 的画布没有背景色。
_Avoid_: 底色, 底图, 背景填充

**区域（region）**:
画布上一个含首尾端点的矩形格子范围。
_Avoid_: 选区, selection, 选框, 范围

**搬运（transplant）**:
把一个区域的内容原样写到另一个位置的动作，分移动与复制两种。
_Avoid_: 拖拽, 位移, 移动粘贴

**移动（move）**:
搬运时擦除源区域，并在原位置写入当前背景色；画布没有背景色时原位置留空格。
_Avoid_: 剪切, 拖动

**复制（copy）**:
搬运时保留源区域，目标位置被整块覆盖。
_Avoid_: 粘贴, 图章, 复制粘贴

**拼豆图纸（pattern sheet）**:
逐格标注规范化色号、带四边 1 基序号与计数线的可打印图纸。
_Avoid_: 图纸导出, chart, 网格图
