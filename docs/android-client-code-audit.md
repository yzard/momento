# Momento Android 客户端代码与设计审计报告

审计日期：2026-08-27\
审计范围：`src/android/app/src/main/`、Android JVM 测试、Android Instrumented 测试\
审计方式：静态代码与架构审查、Git 历史回归追踪、设计系统一致性审查、测试清单检查、编译/JVM 测试/Lint 验证\
不在本次范围：后端实现、Web GUI 功能对齐、生产数据验证、逐设备实机截图对比

## 一、结论摘要

Android 客户端目前功能可用，构建和现有测试正常，但还没有形成一个完整、统一的页面架构。最主要的问题不是某个颜色或 padding 写错，而是以下概念都由不同页面各自管理：

- 页面标题与状态栏 Insets；
- 内容从标题下方开始还是滚动到标题下方；
- Loading / Empty / Error 状态；
- 分页；
- 图片和 thumbnail 加载；
- 媒体选择状态与悬浮操作栏；
- 网络错误转换；
- 列表、详情与 Viewer 的返回关系。

因此，现在即使把某几个页面调成一样，下一次修改也很容易再次漂移。标题就是最明显的例子：Timeline、Trash 的标题由 Shell 绘制；Albums、Places、Admin、Settings 自己绘制；Face detail 仍保留旧式的 grid 内标题。它们没有共享同一个关于标题高度、状态栏、滚动、返回按钮和内容留白的完整契约。

生产 Kotlin 代码约 10,267 行。最大的文件如下：

| 文件 | 行数 | 当前混合的职责 |
| --- | ---: | --- |
| `AdminScreen.kt` | 1,146 | 轮询、用户、导入、Metadata、AI、Cron、响应式 Tab/Rail、Dialogs |
| `Backup.kt` | 1,134 | 扫描、权限、快照、Hash、上传协议、取消、WorkManager、状态转换 |
| `MomentoApplication.kt` | 1,066 | 登录、Server 设置、导航、Drawer、Shell、搜索、页面分发 |
| `ViewerScreen.kt` | 991 | Pager、缩放手势、播放、分享、删除、Filmstrip、Sheet、系统栏 |
| `SettingsScreen.kt` | 812 | 账户、APK 更新、Backup 状态、权限、调度、历史修复、主题 |
| `AlbumsScreen.kt` | 767 | Album 列表、详情、选择、排序、编辑、添加媒体、Collage |

生产代码中共有 113 个针对 `IOException`、`HttpException` 或 `SerializationException` 的重复 catch。这说明请求状态和错误处理正在被每个页面重复实现。

本次审计没有修改 Android 功能代码，只新增本报告。`./build_android_client.sh verify` 已通过 Android 编译、JVM 测试和 Lint。

## 二、严重度定义

- **S1 — 高优先级/结构性问题**：已经导致可见不一致、行为风险，或让相关修改非常容易再次出错。
- **S2 — 重要问题**：造成重复、维护成本、响应式漂移或明显的边缘问题。
- **S3 — 清理问题**：通常不会立刻破坏功能，但会让代码含义不真实、不直接或持续积累债务。

## 三、问题总表

| ID | 严重度 | 范围 | 问题 |
| --- | --- | --- | --- |
| A-01 | S1 | 页面结构 | 标题、Insets、滚动留白和悬浮 Chrome 有多个 owner |
| A-02 | S1 | 导航 | 导航只是 Destination 标志加页面内部 detail 状态，不是统一 Route Stack |
| A-03 | S1 | Viewer 风格 | 黑色 Viewer 中同时存在强制 Dark 和跟随 App Theme 的两套控件颜色 |
| A-04 | S1 | Viewer 逻辑 | Viewer 自动隐藏计时器不会在控件已经显示时可靠重置 |
| A-05 | S1 | 状态管理 | 大型 Composable 直接管理请求、Mutation、分页、Dialog 和业务状态 |
| A-06 | S1 | Capability | Capability 请求失败时 fail-open，会显示服务端可能不支持的页面和 Backup |
| A-07 | S2 | 悬浮控件 | 相同层级的 Floating UI 使用不同 Surface、阴影、Shape 和按压反馈 |
| A-08 | S2 | Detail Viewer | Album、Place、Face 的媒体详情是三套实现 |
| A-09 | S2 | 图片加载 | 类似 thumbnail 展示至少有五套加载管线 |
| A-10 | S2 | 选择模式 | 选择标记与 Selection Action Bar 有多套实现 |
| A-11 | S2 | 分页 | 多套重复分页逻辑并存，同时已有 Paging State 完全未接入生产代码 |
| A-12 | S2 | 错误处理 | 重复 catch 导致异常覆盖不一致并丢失有效错误信息 |
| A-13 | S2 | Admin | Admin 无论当前 Tab 都轮询所有状态，并用全局 Busy 锁住独立 AI 操作 |
| A-14 | S2 | Settings | Settings 是一个大工作流，且在确认有更新前就要求 APK 安装权限 |
| A-15 | S2 | Backup | Backup 将多个架构层和重复状态转换放在一个文件中 |
| A-16 | S2 | 响应式设计 | 各功能自行定义 Breakpoint、网格和顶部留白 |
| A-17 | S2 | 页面状态 | Loading/Empty/Error 组件不完整、使用不统一，部分文案错误 |
| A-18 | S2 | 可访问性 | 大字体、搜索语义、Touch Target 和 Switch 语义未统一处理 |
| A-19 | S2 | 文案 | 所有用户文案与复数规则散落在 Kotlin 中 |
| A-20 | S2 | Map 风格 | Map Marker 和错误提示形成独立的硬编码视觉系统 |
| A-21 | S2 | Core 层 | Repository、API、Models 过于扁平并集中在大文件 |
| A-22 | S3 | Dead Code | 多个生产 Helper 只被测试调用或完全未调用 |
| A-23 | S3 | Collection Helper | 多个 Append/去重 Helper 重复并使用 O(n²) 检查 |
| A-24 | S2 | 测试 | Helper 测试较多，但没有保护完整页面的视觉与交互一致性 |
| A-25 | S1 | Viewer Filmstrip | Thumbnail Filmstrip 在横屏被显式隐藏，且现有带底色实现不符合快速浏览要求 |
| A-26 | S2 | Viewer Info | Media Info 横竖屏共用 Bottom Sheet，没有利用横屏右侧空间 |
| A-27 | S2 | Video Progress | 播放进度条直接使用默认 Material Slider，视觉与 Viewer Chrome 不统一 |

## 四、详细问题

### A-01 — 标题、Insets、滚动留白和悬浮 Chrome 有多个 Owner

**严重度：S1**

证据：

- `Routes.kt:35-38` 通过负面排除列表决定谁不使用 Shell 标题：Settings、Albums、Places、Admin。
- `MomentoApplication.kt:792-872` 在 Destination 内容之外单独绘制 Shell Chrome。
- `AlbumsScreen.kt:138-183`、`PlacesScreen.kt:137-158`、`AdminScreen.kt:258-303`、`SettingsScreen.kt:400-412` 分别自己处理标题和 Insets。
- `FacesScreen.kt:255-285` 仍把返回按钮和 `titleLarge` 标题放在 MediaGrid header 中，没有使用 `MomentoDetailPageHeader`。
- Album 和 Place detail 使用 `momentoMediaViewerContentPadding`；Face detail 使用另一套固定 `top = 64.dp, bottom = 88.dp`。

影响：

- 修改标题需要同时寻找多个实现。
- Shell Header 和页面内 Header 的状态栏处理不同。
- Face detail 可能同时保留 Shell 的 “Faces” 和内部 “Person …” 两个标题层级。
- Loading、Error、Empty、Loaded 状态不一定占用相同顶部空间。
- 新页面很容易继续给 `hasShellPageTitle()` 增加例外。

建议：

建立一个完整的 `MomentoPageScaffold`，而不是继续增加标题 Widget。它应统一负责：

- Safe Drawing / Status Bar Insets；
- Title、Subtitle、Back、Trailing Action；
- 内容位于 Header 下方还是滚动到 Floating Header 下方；
- 统一 Top/Bottom Content Padding；
- Floating Top/Bottom Controls；
- Loading、Empty、Error、Content 的位置。

Shell 应读取当前 Route 明确提供的 Page Chrome，或由 Route 使用同一个 Scaffold。不要继续通过排除列表猜测标题 Owner。

### A-02 — 导航不是统一 Route Stack

**严重度：S1**

证据：

- `MainShellState.kt:11-63` 只保存一个 `Destination`、Viewer 列表/Index 和 `contentRevision`。
- `MomentoApplication.kt:554-556` 将所有非 Timeline 页面的系统 Back 直接送回 Timeline。
- Album、Place、Face 的 Detail ID 分别保存在页面内部，并分别安装 `BackHandler`。
- `MomentoApplication.kt:612-627` 使用 `key(contentRevision)` 在 Viewer 发生修改后强制重建整个 Destination。
- 项目依赖了 Navigation Compose，但没有 `NavHost` 或等价的 Typed Route Stack。

影响：

- Shell 不知道当前是列表还是详情，因此无法正确拥有标题与返回状态。
- Back 逻辑分散在 Shell 和多个 Child Screen。
- Process Recreation 和状态恢复不完整。
- Viewer 修改后的刷新依靠强制重组，而不是明确的数据事件。

建议：

使用一个 Typed Route Model 管理 List、Detail、Viewer。可以采用 Navigation Compose，也可以使用简单的 Sealed Route Stack；关键是标题、返回、内容 Identity、Viewer 返回位置只有一个 Source of Truth。

### A-03 — Viewer 中存在两套 Floating Color Context

**严重度：S1**

证据：

- `ViewerScreen.kt:356` 始终使用黑色画布。
- Viewer Surface 在 `ViewerScreen.kt:658`、`699`、`772`、`833` 显式请求 `darkTheme = true` 的 Floating Colors。
- Close、Share、Delete 使用的 `MomentoFloatingButton` 无法传入 Viewer Style，只会在 `MomentoTheme.kt:164-184` 读取 App 的 `LocalMomentoDarkTheme`。

影响：

Light Theme 下，Viewer 中间控件使用 Dark Floating Palette，但三个圆形按钮使用 Light Palette。同一工具栏会同时出现两种 Container、Outline 和 Contrast。

建议：

Viewer 整体建立一个明确的 Dark Chrome Context。不要在单个控件上分别写 `darkTheme = true`。共享 Floating Component 应统一读取当前 Chrome Style。

### A-04 — Viewer 自动隐藏计时器不会可靠重置

**严重度：S1**

证据：

- 自动隐藏 Effect 只依赖 `chromeVisible`、Media ID、Active Sheet，见 `ViewerScreen.kt:271-275`。
- `userInteracted` 基本只执行 `chromeVisible = true`，见 `ViewerScreen.kt:414` 和播放控件。
- 当 Chrome 已经是 `true` 时，再赋值 `true` 不会触发状态变化，也不会重启 3.5 秒 Effect。

影响：

用户可能正在拖动播放进度、点击播放按钮或操作底部栏，但控件仍按第一次显示时的计时突然消失。

建议：

增加一个明确的 Interaction Revision/Event Flow，由单独的 Viewer Chrome Controller 管理用户活动、Sheet、播放交互和自动隐藏。

### A-05 — 大型 Composable 直接管理业务状态

**严重度：S1**

典型证据：

- `TimelinePageContent`（`TimelineScreen.kt:138-364`）同时处理双向分页、刷新、选择、日期跳转、Dialogs 和删除；文件内有 18 个 Mutable State。
- `AdminScreen.kt` 有 31 个 Mutable State 和 27 个网络异常 Catch。
- `SettingsScreen.kt:196-703` 同时连接 Room、WorkManager、权限、APK 更新、Backup、账户和多个 Dialog。
- `ViewerScreen.kt` 有 23 个 Mutable State，协调 Pager、系统栏、分享、Trash、Zoom、Video、Sheet 和 Chrome。
- `AlbumsScreen.kt` 有 21 个 Mutable State，覆盖 List、Detail、Editor、Selection 和 Add Media。

影响：

- UI 本身变成了没有显式 State Type 的状态机。
- 有些状态用 `remember`，有些用 `rememberSaveable`，有些在 ViewModel，旋转和进程恢复行为不一致。
- 网络逻辑必须通过渲染 Compose 才容易测试。
- 相似页面自然复制同一套状态组合。

建议：

每个 Route 或完整工作流拥有独立 State Owner/ViewModel，暴露 Immutable UI State 和显式 Event。Composable 只渲染并上报用户意图。不要把所有功能塞入一个新的通用 God ViewModel。

### A-06 — Capability Failure 会 Fail-Open

**严重度：S1**

证据：

- `Routes.kt:40-48` 中 Capability 为 null 时，`Destination.isAvailable()` 返回 true。
- `MomentoApplication.kt:569-583` 将 Capability 请求错误转换为 null。
- `MomentoApplication.kt:1041-1046` 使用 `capabilities?.backup?.enabled != false`，即未知状态按可用处理。
- `MomentoApplication.kt:557-567` 静默吞掉 Current User 加载错误。

影响：

离线、响应损坏或 Capability 请求失败时，客户端会显示服务器可能不支持的生成集合和 Backup 控件。UI 无法区分“支持”和“因为加载失败而未知”。Role 相关页面也可能处于不确定状态。

建议：

Capability 必须是明确的 Loading / Available / Failed 状态。Unknown 不应等于全部启用。Current User 与 Capability 加载失败需要可见的 Retry。

### A-07 — Floating Chrome 视觉实现重复

**严重度：S2**

证据：

- `MomentoChrome.kt:115-129` 的共享 `MomentoFloatingDock` 使用 0 dp Shadow。
- Timeline 在 `TimelineScreen.kt:482-563` 自建 3 dp Shadow Surface。
- Trash 在 `TrashScreen.kt:262-324` 自建 3 dp 和 5 dp Surface。
- Faces 和 Deduplicate 在 `FacesScreen.kt:193-210`、`DeduplicateScreen.kt:273-309` 使用全宽 18 dp Card 与 8 dp Shadow。
- Viewer 又建立了一组自己的 Floating Surface。

影响：

相同层级的 Selection/Action Controls 看起来互不相关。按压、Disabled、Border、Elevation 也不一致。

建议：

只保留少量共享 Primitive：Floating Dock、Floating Button、Segmented Selection Control、Destructive Action。Shape、Border、Elevation、Minimum Touch Target、Disabled 与 Press Feedback 只在这些 Primitive 中定义。

### A-08 — Album、Place、Face Detail 是三套实现

**严重度：S2**

证据：

- Album Detail：`AlbumsScreen.kt:242-446`。
- Place Detail：`PlacesScreen.kt:261-336`。
- Face Detail：`FacesScreen.kt:231-287`。

三者都是 Title/Back + Media Grid + 打开 Viewer，但在 Title Ownership、Padding、Loading、Pagination、Subtitle 和 Selection 上分别实现。

建议：

提取一个 `MomentoCollectionDetail` Scaffold。差异必须通过明确参数表达：Title、Subtitle、Paging Source、Selection 能力、Bottom Actions。Face Detail 应完整迁移，不保留旧 Header 作为兼容路径。

### A-09 — 类似 Thumbnail 至少有五套加载管线

**严重度：S2**

证据：

1. `MediaComponents.kt:107-120`：共享 Media Thumbnail 解析 Authenticated URL。
2. `AlbumsScreen.kt:653-720`：Album Collage 自己解析最多四个 URL 并创建 `AsyncImage`。
3. `PlacesScreen.kt:77-86`、`210-259`：请求 Base64 Data URL，解码成 ByteArray 后显示。
4. `FacesScreen.kt:289-349`：请求 Raw Bytes 后单独显示。
5. `NativeMapScreen.kt:410-516`：手动执行 Coil、转 Bitmap、再绘制自定义 Drawable。

影响：

Authentication、Cache、Placeholder、Error、Retry、Crop 和 Content Description 各不相同。部分失败会静默变成空白图片。

建议：

建立统一的 Authenticated Image Source 与 `MomentoAsyncImage`。它可以接受 Media URL、Endpoint Bytes 或预加载 Bitmap，但 Placeholder、Error、Crop 和 Semantics 必须共享。Map 可以在取得统一加载结果后继续做专用 Marker 绘制。

### A-10 — Selection Visual 与 Action Bar 重复

**严重度：S2**

证据：

- `MediaComponents.kt:122-146`：通用 Media Selection Overlay。
- `FacesScreen.kt:310-347`：Face Icon + Card Color。
- `DeduplicateScreen.kt:382-421`：另一套 Media Selection。
- `TimelineScreen.kt:506-563`：Timeline Selection Toolbar。
- `TrashScreen.kt:262-324`：Trash Selection Toolbar/Actions。
- `AlbumsScreen.kt:573-605`：Album Selection Dock。

影响：

同一个 Selected/Selection Mode 概念使用不同 Icon Contrast、Count 文案、退出行为和 Action Placement。

建议：

共用 Selection Indicator 和 Selection Action Dock。功能差异通过 Action List/Data 传入，而不是重新建立 Surface。

### A-11 — 分页重复，已有 Paging 抽象却未接入

**严重度：S2**

证据：

- Places、Faces、Deduplicate、Place Detail、Timeline 都独立维护 Cursor、Loading、Error 和 Append。
- Places/Faces 同时使用接近底部自动加载和手动 “Load more”；Deduplicate 与 Place Detail 又使用不同手动策略。
- `MediaPagingState.kt` 定义了 `MediaPagingState`、`appendMediaPage`、`mediaPageFailed`，但生产页面没有一个调用。
- `AdaptiveGrid.kt:30-35` 已有近底部 Predicate，Timeline 又在 `TimelineScreen.kt:108-113` 复制了相同判断。

影响：

分页与 Retry UX 在不同集合中不一致。未完成的抽象与真实生产路径并存，是典型的 Patch-on-Patch 痕迹。

建议：

选择一个 Paging State Model，并在同一次改动中迁移全部单向分页集合。Timeline 的双向分页可以保留专用实现，但应复用相同 Request State Primitive。迁移后删除旧路径。

### A-12 — 网络错误处理重复且覆盖不一致

**严重度：S2**

证据：

- Android 生产代码中有 113 个常见网络/序列化异常 Catch。
- 部分 Admin Delete/Import/Metadata 路径只 Catch IOException/HttpException，没有统一覆盖 SerializationException。
- `AlbumsScreen.kt:758-767` 的 `executeAlbumOperation()` 将错误压成 Boolean，丢失 HTTP 状态与原因。
- MainShell 静默吞错，其他页面有的用 Dialog、有的用 Inline Text。

影响：

新增请求很容易漏掉某类异常；用户反馈不一致，日志与 Retry 判断也缺少信息。

建议：

在 UI 下层统一做 Typed Request Error Classification。页面决定如何展示，但不再重复 Transport Exception List，也不把所有错误压成 Boolean。

### A-13 — Admin 轮询和 Busy 范围过大

**严重度：S2**

证据：

- `AdminScreen.kt:196-255` 每三秒并发刷新 Import、Metadata、AI，不管当前显示哪个 Tab。
- `DeduplicateScreen.kt:202-208` 另外每两秒请求一次 Aggregate AI Status。
- `AdminScreen.kt:763-803`、`835-933` 使用一个全局 `busyControl`，任何一个 AI 操作都会禁用所有独立 Feature Control。
- 四个 Admin Section 及其 Dialog 全部放在同一个文件。

影响：

产生不必要的请求与 Recomposition；UI 声称任务独立，但在客户端被全局串行化；文件很难单独测试与修改。

建议：

每个 Admin Section 有自己的 State Owner 和 Screen Module。只轮询可见 Section，或由一个 Lifecycle-aware Status Stream 给多个消费者。Busy State 应按 Action Key 管理，除非后端明确要求全局串行。

### A-14 — Settings 混合无关工作流，更新权限请求过早

**严重度：S2**

证据：

- `SettingsScreen.kt:196-703` 同时处理账户、更新、Backup、权限、History、Theme、Admin Navigation。
- `SettingsScreen.kt:294-305` 在下载并确认有新版本之前，先要求 Unknown App Source 安装权限。
- `SettingsScreen.kt:307-379` 在 Composable Coroutine 中协调文件清理、Package 解析、Staging 和 Installer。
- `SettingsScreen.kt:467-536` 将大量 Backup 状态和按钮嵌套在一个 `ListItem` Supporting Content 中。

影响：

用户只是检查版本，也必须先授予敏感安装权限。页面信息层级弱，且任何 Backup/Update 修改都会触碰同一个大型函数。

建议：

拆分 Account、Update、Backup、Appearance Section 与各自 State Owner。先下载并验证 APK；只有确认存在合法新版本时再请求安装权限。文件和 Package 操作移动到 IO Update Coordinator。

### A-15 — Backup 将多个架构层放在一个文件

**严重度：S2**

`Backup.kt` 同时包含：

- MediaStore 扫描；
- 权限判定；
- 网络 Constraints/Observer；
- Protected Snapshot 与 Metadata；
- Hash 和 Chunk Upload；
- Server Upload State Transition；
- Cancellation 与 Cancellation Worker；
- WorkManager 调度；
- Notification；
- History Clear/Repair。

`Backup.kt:287-437` 的 `transfer()` 还为 `TerminalBackupException`、`SecurityException`、`IllegalArgumentException` 保留了三段几乎相同的终止与取消转换。

影响：

Backup 的领域复杂度本身是必要的，但当前文件结构让必要复杂度与重复复杂度混在一起。一个 Concern 的改动需要理解并回归其他全部 Concern。DAO 状态转换也通过长参数列表重复调用。

建议：

按 Scanner、Snapshot Store、Transfer State Machine、Cancellation、Scheduler、Permissions、Workers 拆分。使用 Typed Transition Object 替代重复的八参数 `updateTransfer`。这是结构重构，不应改变 Durable Backup Protocol。

### A-16 — Responsive Breakpoint 与 Padding 各自定义

**严重度：S2**

证据：

- General Media Grid：`AdaptiveGrid.kt:15-22` 使用 360/480/600/840/1200。
- Places Grid：`PlacesScreen.kt:69-75` 使用 360/600/840/1200。
- Deduplicate：`DeduplicateScreen.kt:80-84` 使用 560/840。
- Admin Rail：`AdminScreen.kt:137` 使用 720。
- Cron Fields：`AdminScreen.kt:131-135` 使用 420/720。
- Compact Action：Timeline 使用 480，Deduplicate 使用 460。
- 顶部留白在页面中分别硬编码 64、68、88、104 dp。
- `momentoWindowClass()` 与 `adaptiveContentPadding()` 已存在，但后者没有生产调用。

影响：

Portrait、Landscape、Split Screen、Tablet、大字体在不同页面跨过不同阈值。Content Padding 也没有根据真实 Header 高度与 Insets 计算。

建议：

建立一套 Window Class 和 Content Width/Gutter/Grid Density Token。媒体顶部 Padding 由 Page Scaffold 产生。功能专属 Breakpoint 应非常少并有明确理由。

### A-17 — Loading/Empty/Error 不统一，部分 Empty Copy 错误

**严重度：S2**

证据：

- `UiStates.kt` 有共享组件，但 Albums、Admin、Map、Viewer 又自己实现。
- `UiStates.kt:30-39` 的 `EmptyState` 无论页面都追加 “New memories will appear here when they are available.”。
- Trash 使用 `EmptyState("Trash is empty")`，因此空 Trash 也会说新 Memories 会出现在 Trash。
- `DeduplicateScreen.kt:242-244` 将 `fillMaxSize()` Empty State 放进 Lazy Item，与全屏 Empty State 不是同一布局语义。
- `UiStates.kt:59` 的 `ConfirmDialog` 没有任何生产调用。

建议：

State Component 应显式接受 Title、Explanation、Icon、Action，并区分 Full-page 与 Inline。未使用的 Confirm Wrapper 要么完整迁移调用者，要么删除。

### A-18 — Accessibility 与 Dynamic Type 未形成契约

**严重度：S2**

证据：

- `MomentoChrome.kt:44-90` 将 Title 和 Subtitle 都限制为单行，Trailing Action 会进一步压缩标题。
- `MomentoApplication.kt:908-999` 使用自定义 `BasicTextField` 做搜索，但没有稳定 Label 或明确 Search Field Semantics。
- `AlbumsScreen.kt:634-649` 的 Delete Overlay 被限制为 40 dp，低于常用 48 dp Touch Target。
- `SettingsScreen.kt:723-730` 将 Switch 行为拆在 Row Clickable 和 `onCheckedChange = null` 的 Switch 上，需要验证 TalkBack 合并后的 Checked State。
- Map Marker 通过 osmdroid Listener 交互，没有 Compose Semantics 测试。
- 没有大字体完整页面测试。

建议：

测试 1.0x、1.3x、1.5x Font Scale；标题允许增长或在窄宽度重新布局 Trailing Action；Custom Field/Control 添加明确 Semantics；统一 Minimum Touch Target；为 Selection、Search、Switch、Map、Viewer 添加 TalkBack 测试。

### A-19 — 用户文案散落在 Kotlin

**严重度：S2**

证据：

- Android Resources 中没有产品 `strings.xml` 与 Plurals。
- `ViewerScreen.kt:153-159` 强制 `Locale.ENGLISH`。
- Memory、Item、Media、People 等复数逻辑在多个文件手动拼接。

影响：

无法干净本地化，复数规则仅适合英语，产品文案也无法集中 Review。

建议：

所有用户文本和 Accessibility Description 移入 String/Plural Resource。Server Identifier 与内部 State Value 保持在代码中。

### A-20 — Map 使用独立硬编码视觉系统

**严重度：S2**

证据：

- `NativeMapScreen.kt:433-516` 的 `ClusterMarkerDrawable` 硬编码浅 Slate、深 Navy、白色 Border、Shadow、文字大小和 Geometry。
- Marker 不读取 Momento Light/Dark Theme。
- `NativeMapScreen.kt:352-363` 的错误 Surface 只使用 raw `top = 12.dp`，没有 Safe Insets；Shell Title 又在 Map 外层单独绘制。

影响：

Marker 不随 Theme/Contrast 调整，错误提示可能与状态栏或 Shell Title 竞争位置。

建议：

建立由当前 Theme 生成的 Map Visual Tokens，并传入 Drawable。Map Error 通过共享 Page Scaffold 的 Status/Overlay Region 展示。

### A-21 — Core Network/Model 过于扁平

**严重度：S2**

证据：

- `MomentoRepository.kt` 同时暴露 Auth、Timeline、Albums、Places、Faces、Trash、Map、Admin、AI、Download、Backup。
- `MomentoApi.kt` 在一个 Interface 中包含全部 Server Contract。
- `Models.kt` 将 Auth、Media、AI、Admin、Map、Backup DTO 压缩在一个文件，并大量使用单行声明。

影响：

Core 层没有清楚的 Feature Boundary；大型文件容易冲突；Feature State Owner 不能依赖 Narrow Interface。

建议：

按资源层级拆分 API、Repository Interface 和 Models，同时保留一个共享 Authenticated Client。不要复制 Retrofit 或 Token Logic。

### A-22 — 生产代码包含 Dead/Test-only Helper

**严重度：S3**

没有生产调用的例子：

- `MediaPagingState.appendMediaPage()`、`mediaPageFailed()`；
- `adaptiveContentPadding()`；
- `removeFromAlbumSelectionLabel()`；
- `toggledRole()`；
- `serverProgress()`；
- `canManageDeduplication()`。

影响：

这些函数让人误以为生产代码正在使用某个统一抽象；实际上测试通过的是一条与真实页面不同的路径。

建议：

要么在一次完整迁移中让生产代码真正使用它，要么删除 Helper 和对应测试。不要把 Test-only Helper 当作生产文档。

### A-23 — Collection Append Helper 重复且为 O(n²)

**严重度：S3**

证据：

- `appendPlaceMedia`、`appendPlaces`、`appendFaceGroups`、`appendDeduplicateGroups` 都对每个 Candidate 执行 `existing.none`。
- Timeline 使用另一套 Group/ID Merge。
- 多个 Media Click Handler 再执行 `indexOf` / `indexOfFirst` 查找当前 Index。

影响：

逻辑重复，列表增长后为 Quadratic。当前它不是最大性能瓶颈，但完全可以消除。

建议：

使用一个 Keyed Append Helper，预先建立 Existing Key Set。Lazy Item 可以直接提供 Index 时，不再点击后重新搜索列表。

### A-24 — 测试没有保护完整页面一致性

**严重度：S2**

现有优点：

- Pure Helper 和 DTO Contract 的 JVM 测试较多。
- Admin Portrait/Landscape Tab/Rail 切换有 Instrumented Test。
- Shared Chrome、Album Action Order、Password Fields、Room Migration、Long Media Grid 有 Instrumented Test。

主要缺口：

- Timeline、Places、Faces、Trash、Settings、Viewer、Map 没有完整页面 Instrumented 或 Screenshot/Golden 测试。
- 没有 Light/Dark/System Theme Matrix。
- 没有 Compact Portrait、Compact Landscape、Tablet Portrait、Tablet Landscape 的全页面 Matrix。
- 没有 Large Font、TalkBack、Offline、Retry、Rotation State 测试。
- Viewer Chrome Timing、Playback Interaction、Zoom/Pager Handoff、Mixed Theme Control 没有 UI 测试。
- Viewer Filmstrip 的横竖屏可见性、快速 Fling、Pager 同步、两端 Fade 条件没有 UI 测试。
- Media Info 没有验证竖屏 Bottom Sheet、横屏 Right Side Panel 及旋转时的内容/打开状态保持。
- Video Seek 没有验证拖动期间 Chrome 保持、松手后单次 Seek、圆点 Thumb 与最小触控区域。
- 没有跨 Destination 的 Title/Background/Insets 一致性测试。

建议：

先测试 Shared Scaffold Contract，再给每个页面建立小型 Screenshot/Semantics Matrix。动态照片不需要脆弱的全像素 Snapshot；应验证 Chrome Bounds、Typography Role、Background Token、Semantics 和固定 Fixture Image。

### A-25 — Thumbnail Filmstrip 的“消失”是可定位的横屏回归，现有视觉也不符合目标

**严重度：S1**

Git 历史结论：

- `93290bd`（2026-08-24，`Optimize android client`）首次加入 `ViewerFilmstrip`，并在 `ViewerBottomControls` 中无条件渲染。
- `30ff657`（2026-08-27，`Fixes`）加入 `compactLandscape = maxWidth > maxHeight`，随后用 `if (!compactLandscape)` 包住 `ViewerFilmstrip`。
- 当前 `ViewerScreen.kt:708-716` 仍保留这个条件。因此 Filmstrip 没有从代码中删除，而是对所有 Landscape 窗口明确关闭。
- Filmstrip 位于整个 Bottom Chrome 的 `AnimatedVisibility` 内；Chrome 自动隐藏时，它在 Portrait 也会一起消失。结合 A-04 的计时器问题，用户操作 Filmstrip 或播放控件时还可能遇到过早淡出。

现有实现问题：

- `ViewerScreen.kt:794-799` 用一个有 Container Color 的大型圆角 `Surface` 包住整条 Filmstrip，形成明显的胶囊背景。
- `ViewerScreen.kt:795` 再加 68 dp 水平留白，窄屏可用浏览范围偏小。
- 当前没有左右 Edge Fade，用户无法从视觉上判断两端是否还有更多媒体。
- 停止滚动后，居中的 Thumbnail 会立即驱动 Pager；Pager 更新 `currentIndex` 后又触发 `animateScrollToItem`。这是双向状态同步，但 Owner 不明确，后续很容易产生回弹、二次动画或与快速 Fling 竞争。

已确认的目标：

- 媒体集合多于一项时，Portrait 与 Landscape 都必须提供 Filmstrip；横屏不能通过隐藏功能来解决空间问题。
- Filmstrip 本身无 Card、胶囊或整条背景，只显示 Thumbnail、选中态和必要的边缘层次。
- 支持快速连续左右拖动与带速度的 Fling；滚动必须贴手、可立即反向，不因 Pager 更新被强制拉回。
- 左侧还有媒体时仅左边 Fade Out，右侧还有媒体时仅右边 Fade Out；到达对应边界后 Fade 必须消失。Fade 是内容边缘 Mask，不是额外容器背景。
- 当前媒体使用克制的白色圆角描边或轻微尺寸/亮度变化表示；未选项不增加重复边框。
- Filmstrip、Pager 和当前媒体只由一个 Navigation/Selection Controller 同步，避免 `LazyRow -> Pager -> LazyRow` 的反馈环。

建议：

将 Filmstrip 从 `ViewerBottomControls` 的背景 Surface 中拆成独立的 `ViewerFilmstrip` Overlay。用 `LazyListState.canScrollBackward/canScrollForward` 或等价的 LayoutInfo 派生左右 Fade；滚动过程中更新本地候选项，滚动停止或点击后通过一个明确入口提交导航。Filmstrip 交互期间必须暂停并重置 Chrome 自动隐藏计时。

### A-26 — Media Info 缺少横屏 Right Side Panel

**严重度：S2**

证据：

- `ViewerScreen.kt:420-436` 对 Album Picker 和 Media Information 都固定使用 `ModalBottomSheet`。
- `compactLandscape` 只传给 Bottom Controls；Sheet Presentation 不读取窗口方向或可用宽度。
- 横屏 Bottom Sheet 从底部占据媒体最宝贵的垂直空间，却留下可利用的左右空间。

影响：

- Landscape 中媒体被大面积遮挡，Information 与画面的空间关系不自然。
- 旋转只改变底层 Viewer 布局，不会把已经打开的 Information 转换成适合方向的 Presentation。
- 如果直接分别实现 Bottom Sheet 和 Side Sheet，Media Info 内容、错误、滚动和关闭逻辑会再复制一套。

已确认的目标：

- Portrait：Media Info 从底部弹出。
- Landscape：Media Info 从右侧滑入，并从右侧退出；保留尽可能大的媒体可视区域。
- 两种布局复用同一个 `MediaInformationContent`、同一个打开状态和同一套数据，不复制功能代码。
- 旋转时保持 Info 打开及滚动/内容状态，只替换 Presentation Container。
- Back、Scrim 点击和明确 Close 都使用同一个关闭事件；动画应可中断，不在转场期间锁住输入。

建议：

建立一个 `AdaptiveViewerSheet`：根据当前 Window Size/Orientation 选择 Bottom Sheet 或 Right Side Sheet，内容通过 Slot 注入。Album Picker 是否也采用横屏 Side Sheet 应作为同一 Primitive 的显式 Presentation 参数，而不是再写第二套窗口判断。

### A-27 — Video Progress 仍是默认 Material Slider，不是 Viewer 的专用播放控件

**严重度：S2**

证据：

- `ViewerScreen.kt:854-911` 用一个大型有底色圆角 `Surface` 包住 Play、默认 `Slider`、时间和 Volume。
- `ViewerScreen.kt:876-892` 没有提供自定义 Track、Thumb 或视觉尺寸，外观完全由 Material 组件默认值决定；当前版本的 Thumb 呈竖向 Handle，不符合圆点 Cursor 要求。
- 现有代码在 `onValueChange` 中只更新 Preview，并在 `onValueChangeFinished` 中执行一次 `seekTo`；这是正确的流媒体 Seek 边界，重设计时应保留。
- 播放进度每 200 ms 轮询一次，但 Viewer Chrome 的存活时间没有用明确的 Drag Session 管理，见 A-04。

已确认的目标：

- Progress Track 是细长水平线，Played、Buffered（如可取得）和 Remaining 使用统一 Dark Viewer Chrome Token。
- Cursor/Thumb 是圆点，不是竖线；视觉尺寸克制，但不可把触控范围缩小到视觉尺寸，仍需保留至少 48 dp 高的透明交互区域。
- 拖动时进度和时间预览必须 1:1 跟手，控件保持可见；松手后只执行一次真正的 `seekTo`，取消拖动则恢复真实播放位置。
- Play/Pause、时间、Progress、Volume 的排版应属于同一个 Viewer Control System，但不再依赖笨重的整条胶囊背景。
- Portrait 与 Landscape 共用同一个行为实现，只调整可用宽度和信息密度。

建议：

提取 `ViewerProgressControl`，显式定义 Track、Buffered Track、Played Track、圆形 Thumb、手势状态和 Semantics。视觉 Track 可以是 2–3 dp，圆点可以保持轻量，但 Hit Area 必须独立满足触控要求。拖动生命周期向 Viewer Chrome Controller 发送 Start/Move/Finish/Cancel，而不是反复把 `chromeVisible` 赋值为 `true`。

## 五、已确认的跨功能重复关系

| 产品概念 | 当前实现 | 应有的单一 Owner |
| --- | --- | --- |
| Page Title + Safe Insets | Shell、Album、Place、Face Detail、Admin、Settings | Shared Page Scaffold + Typed Route Chrome |
| Media Collection Detail | Album、Place、Face | Shared Collection Detail Scaffold |
| Raw Media Grid | MediaGrid、Timeline、Trash、Face Detail | Shared Raw Media Grid |
| Thumbnail | Media URL、Album URL、Place Base64、Face Bytes、Map Bitmap | Authenticated Image Source + Shared Renderer |
| Selection Indicator | Media Overlay、Face、Deduplicate | Shared Selectable Thumbnail/Card Overlay |
| Selection Action Bar | Timeline、Trash、Album、Faces、Deduplicate | Shared Action Dock + Feature Actions |
| Paging | Timeline、Places、Place Detail、Faces、Deduplicate | Shared One-way Pager + Specialized Timeline Pager |
| Loading/Error/Empty | Shared UiStates + Album/Admin/Map/Viewer Variants | Shared Full-page/Inline State System |
| Request Error Mapping | 几乎每个 Screen 重复 Catch | Typed Error Below UI |
| Destructive Confirmation | 多个 AlertDialog + 未使用的 Wrapper | Shared Explicit Destructive Dialog Style |
| Responsive Breakpoints | General、Places、Deduplicate、Admin、Cron、Selection | Shared Window Class + Layout Tokens |
| Viewer Filmstrip + Pager Sync | LazyRow 自己选中、Pager 自己翻页、Chrome 自己隐藏 | Viewer Navigation/Chrome Controller + Background-free Filmstrip |
| Viewer Adaptive Sheet | Portrait/Landscape 共用 ModalBottomSheet | Shared Content + Bottom/Right Adaptive Presenter |
| Video Seek | 默认 Material Slider + Viewer 局部状态 | Shared Viewer Progress Control + Explicit Drag Session |

## 六、建议的目标结构

目标是减少概念，而不是增加更多薄 Wrapper。

### Presentation

- `MomentoPageScaffold`：Title、Back、Subtitle、Insets、Floating Chrome、Page State。
- `MomentoCollectionDetail`：共享 Title + Paged Media Grid 契约。
- `MomentoMediaGrid`：一个 Raw Media Layout，使用显式 Density Mode。
- `MomentoCollectionCard`：Album/Place 的共享视觉基础。
- `MomentoSelectionDock`：Count、Done/Close 和传入 Actions。
- `MomentoAsyncImage`：Authenticated Source、Loading、Failure、Crop、Semantics。
- `ViewerFilmstrip`：无背景 Thumbnail Strip、条件 Edge Fade、快速 Fling 与单向导航提交。
- `AdaptiveViewerSheet`：同一内容在 Portrait Bottom Sheet 与 Landscape Right Side Sheet 间切换。
- `ViewerProgressControl`：细 Track、圆点 Thumb、独立 Hit Area、Seek Preview 与 Drag Session。
- `PageState`：Loading、Empty、Error、Content、Refreshing，不再用 Nullable List 的组合推导。

### State

- 一个 Shell 可见的 Typed Route Stack。
- 每个 Route/Workflow 一个 Immutable UI State + Event Interface。
- 一个共享 One-way Cursor Pager。
- 一个共享 Request Error Classifier。
- 一个独立于 Pager/Player 的 Viewer Controller，统一 Chrome Activity、Filmstrip/Pager Selection、Seek Drag 与 Adaptive Sheet State。

### Data / Domain

- 按 Feature/Resource 拆分 Narrow Repository Interface，共享一个 Network Client。
- Backup 拆为 Scanner、Snapshot Store、Transfer State Machine、Cancellation、Scheduling。
- APK Update Coordinator 与 Settings UI 分离。
- Admin Section 使用各自 State Owner 和 Lifecycle-aware Polling。

## 七、建议实施顺序

### Phase 1 — 建立统一页面契约

1. 以当前 Trash/Header 风格建立 `MomentoPageScaffold`。
2. 用 Route Chrome 替换 `hasShellPageTitle()` 排除列表。
3. 同时迁移 Timeline、Trash、Album List/Detail、Places List/Detail、Face Detail。
4. 添加 Compact/Expanded、Light/Dark、Large Font Scaffold Test。

完成标准：所有标准页面与详情页的 Title Size、Background、Safe Insets、Content Padding 只来自一个实现。

### Phase 2 — 统一媒体展示

1. 建立 Authenticated Image Source/Renderer。
2. 迁移 Media、Album Collage、Place、Face、Map Thumbnail。
3. 建立共享 Selection Indicator 与 Action Dock。
4. 迁移 Timeline、Trash、Album、Faces、Deduplicate Selection。

完成标准：Image Loading 和 Selection 只有一套 Visual/Accessibility Contract。

### Phase 3 — 替换重复 Request/Paging State

1. 定义 Typed Request Error 与 Page State。
2. 完成 Paging 抽象并删除未使用的 `MediaPagingState` 路径。
3. 迁移 Places、Place Detail、Faces、Deduplicate。
4. 用同一 Request Primitive 重构 Timeline 双向 Pager。

完成标准：Screen 中不再重复 Transport Exception List，不再使用 Nullable List + Loading + Error 的组合状态。

### Phase 4 — 简化 Navigation 与 Viewer

1. 将 List/Detail/Viewer Identity 移入 Typed Route Stack。
2. 删除页面内部 Detail `BackHandler` 与 `contentRevision` 强制重建。
3. Viewer 统一使用一个 Dark Chrome Context。
4. 恢复 Portrait/Landscape Filmstrip，去掉整体背景，并加入基于可滚动方向的左右 Edge Fade。
5. 用 Adaptive Viewer Sheet 实现 Portrait Bottom Info 与 Landscape Right Info，不复制 Information Content。
6. 用专用细 Track + 圆点 Thumb Progress Control 替换默认 Material Slider。
7. 提取并测试 Viewer Controller，统一 Chrome Visibility、Filmstrip/Pager、Seek Drag 与 Sheet State。

完成标准：Shell 始终知道当前 Title/Back State；Viewer 不混色；Filmstrip 在横竖屏均可快速浏览且边缘提示准确；Info 从符合方向的一侧出现；拖动进度时控件不会消失且只在结束时提交一次 Seek。

### Phase 5 — 拆分大型工作流

1. Admin 按 Section 拆分，并只轮询可见或共享的状态。
2. Settings 拆为 Account、Update、Backup、Appearance。
3. APK 处理移入 Update Coordinator，并延后安装权限请求。
4. Backup 按 Scanner/Snapshot/Transfer/Cancellation/Scheduler 拆分。
5. Core API/Repository/Models 按 Feature Resource 拆分。

完成标准：Composable 不再协调互不相关的 Workflow；Backup Durable Behavior 由 Transition-level Test 保护。

### Phase 6 — 完整视觉 QA

每个主要页面测试：

- Compact Portrait；
- Compact Landscape；
- Tablet/Expanded Portrait；
- Tablet/Expanded Landscape；
- Light、Dark、System Theme；
- Normal 与 Large Font；
- Loading、Empty、Error、Content、Refreshing、Selection、Working；
- TalkBack Role/State Description；
- Dialog、Detail、Viewer 的 Rotation/State Restoration。
- Viewer Filmstrip 快速 Fling、左右边界 Fade、Pager 同步与 Chrome Auto-hide；
- Video Seek Drag/Cancel/Release、细 Track/圆点 Thumb Semantics；
- Media Info Portrait Bottom / Landscape Right 以及打开状态下旋转。

## 八、需要 Review 的产品决定

以下问题会影响共享契约，实施前需要明确：

1. Settings、Admin 是否也必须使用与 Trash 完全相同的 Bare `headlineLarge` 标题，还是允许一个明确、独立但统一的 Form/Admin Page Category？
2. Face Detail 是否与 Album/Place 一样，让 Media 滚动到 Floating Title 下方？建议是。
3. Viewer 是否无论 App Theme 都固定使用 Dark Chrome？当前黑色 Viewer Canvas 表明应当如此。
4. Collection Paging 统一为自动加载、手动加载，还是自动加载 + Error Retry Row？当前三种都存在。
5. APK Update Check 是用户手动触发，还是后台检查？无论哪种，都不应在确认存在新版本前请求安装权限。

## 九、验证状态

- Android Compile：通过。
- JVM Unit Tests：通过。
- Android Lint：通过。
- Instrumented Tests：本次只做清单审计，没有重新运行。
- 编译器、Unit Test 与 Lint 均无法发现本报告中的页面所有权、混合主题、计时器和视觉一致性问题。
