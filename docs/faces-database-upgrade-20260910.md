# NAS 手动升级 Faces 数据库（2026-09-10）

这是针对本次 Faces 误检排除功能的独立一次性脚本，不会被应用启动流程调用，
也不是通用 migration 框架。只支持此次变更前、已有当前 Faces 分组与文件清理表的数据库。
脚本只依赖 Python 3.8+ 标准库，可单独复制到 NAS，无需复制仓库或安装 Python 包。

脚本：`src/backend/maintenance/faces/upgrade_20260910.py`。

## 操作

1. 停止 Momento API 容器及任何其他会访问同一数据库的 Momento 实例，暂不启动新版。
   仓库默认容器名可用 `docker stop momento-api`；若 NAS 使用不同名称，请使用实际名称。
2. 确认配置中 `server.data_dir` 映射到 NAS 的目录。数据库为该目录内的
   `database.sqlite`，不是 llm-service 的数据库。预留至少一个完整数据库备份的可用空间。
3. 将脚本复制到 NAS。先检查，再执行。下面路径仅为示例，请替换成实际路径：

```bash
python3 upgrade_20260910.py --database /volume1/docker/momento/data/database.sqlite

python3 upgrade_20260910.py \
  --database /volume1/docker/momento/data/database.sqlite \
  --apply --services-stopped \
  --backup /volume1/docker/momento/database.before-faces-20260910.sqlite
```

执行账号需要读写数据库目录和备份目录。默认仅检查，不修改数据库。
`--services-stopped` 是操作者对停机状态的声明，脚本无法检测所有容器或远端进程。
脚本持有 SQLite 写锁直至提交；锁占用、缺少前置结构或完整性检查失败会直接停止。

成功输出 `Upgrade complete` 后，部署并启动新版 Momento API，配套更新 WebGUI 和
Android 客户端。不要执行 Clean AI Face Data；本次升级无需清空现有人脸或分组。
备份包含用户数据和认证记录，请限制访问并妥善保存。

## 具体变更和保护

- 新增 `media_faces.frame_timestamp_ms`。图片保持 null；视频从同一媒体、同一
  `face_detection` 输入序号的已保存帧描述记录回填时间戳，不根据帧率猜测。
- 新建 `face_rejections`、`face_rejection_operations`、匹配索引和排除触发器。
- 不删除媒体、人脸、分组、访问授权或 AI 输入。由于应用严格比较建表 SQL，
  脚本在事务内重建 media_faces，使字段顺序和定义完全匹配新版；原样复制所有
  记录、保留自增序号和分组引用，恢复索引与触发器。提交前校验外键和完整 schema 指纹。
- 使用 SQLite backup API 备份，包括已提交的 WAL 数据；禁止覆盖现有备份文件。
  备份通过完整性检查后才进行更新。不要在运行中的数据库上用普通文件复制代替它。
- 更新在单个事务中完成；异常自动回滚，已经验证成功的备份保留。
- 相同版本重复运行会显示 `Already upgraded`，不修改数据、不重写备份。
  已有对象定义不符合此次版本时拒绝执行。

如果旧视频人脸的帧描述记录已丢失，脚本会报告数量并在修改前停止。
此时保留数据库和输出，先恢复对应的 `media_ai_inputs` 记录；脚本不通过删除人脸或
把视频帧当作图片来绕过检查。若结构比此次变更前的版本更旧，需要另行核对差异。

## 恢复备份

通常失败会自动回滚，无需恢复。如果升级后需要恢复，先停止所有数据库使用者，
将当前 `database.sqlite` 及存在的 `database.sqlite-wal`、`database.sqlite-shm`
一起移到独立保留目录，再将已验证的备份复制为原路径的 `database.sqlite`，
保持原服务账号所需的文件权限。不要让旧 WAL 与恢复后的数据库混用。
恢复旧结构后使用升级前的程序版本；重新升级成功后才启动新版。

## 本地验证

测试仅使用 `build/tmp/` 内的临时数据库：旧结构升级、WAL 备份、视频时间戳回填、
误检触发器、保留分组、重复执行、禁止覆盖备份、不兼容结构拒绝、缺失帧描述拒绝、
缺失数据库不创建，以及事务失败回滚、修复 ALTER TABLE 列顺序和完整 schema 比对。

```bash
mkdir -p build/tmp
export TMPDIR="$PWD/build/tmp" TEMP="$PWD/build/tmp" TMP="$PWD/build/tmp"
python3 -B -m unittest discover -s tests/backend/maintenance/faces -v
```
