# SubVault T0 / T1

这是可丢弃的合成验证，不是可试用产品。保护方式和主要输入都还没有选定。这里的明文库只用于测试，安全设计没有完成。

T0 准备虚构原文和两份独立预期。T1 只证明一件事：原件字节被单独托管，并且失败时不会假装已经保全。T1 不解析订阅、不判断金额、不建立购买或权益，也不提供整理、全文搜索或提取。

## T0

`synthetic/generate_corpus.py` 用标准库写出真实的 `%PDF-1.4` 文件、一份 CSV，以及一条禁止规范化的字节试样。`synthetic/corpus/byte-expectations.json` 记录路径、角色、媒体类型、长度和 SHA-256。`synthetic/corpus/logical-expectations.json` 记录月/年付、同商户双账户、一次性购买原文范围、Total 退款、未判订单总额、旧服务期结束但当前未知、取消续订仍可用、掩码未归属、金额冲突、人改重导。文件里的 `t1_implements_these_judgments` 是 `false`：保全这些字节不等于做出这些判断。

复核锚点日期是 2026-11-14。商户、邮箱和订单号都是虚构的。不要把改名文本当成 PDF；生成器会检查 `%PDF-1.4` 和 xref。

```sh
python3 synthetic/generate_corpus.py --check
```

## T1 在做什么

本地一个目录就是一个库。`subvault` 是唯一支持的操作面。测试通过这个可执行文件启动子进程，检查退出码、JSON、落盘字节和重新打开后的状态。

每个库在创建时写入固定的 `SYNTHETIC_ONLY` 横幅。横幅缺失时打开失败，程序不会补建空库。横幅被改过时拒绝写入，也不会把它改回去。

一次成功的导入：

- 用随机的 `src_` 源身份指代这份原件。身份不是路径、文件名或内容哈希。
- 把原始字节复制进 `objects/obj_<32 hex>`。对象名是内部身份，不是哈希，也不是原文件名。
- 相同字节只保留一份对象。另一条路径或另一个文件名会新增一次 `occ_` 出现，并复用对象；这不会合并购买或权益，因为本层没有购买表。
- 命令以调用者给出的 CommandId 记入 SQLite。同一 CommandId 再次执行时，先核对托管对象的当前字节。对象缺失或字节被改过时返回恢复所需，`new_effect` 为 false，不复制、不覆盖、不补回旧对象。托管字节仍完整时：外部路径已经不存在，返回当时的 `AlreadyCommitted`，`source_check` 为 `absent`，不再核对媒体类型，因为没有字节可读；路径还在但读不出来，返回 `source_unreadable`，这不是新的导入成功；路径可读且内容或媒体类型不同，返回 `payload_conflict`。显示名不在指纹里，只改显示名仍是 `AlreadyCommitted`，并保留第一次的显示名。
- 回执里的 `preservation` 只有在 SQLite 提交成功之后才是 `preserved`。提交成功后的 journal、检查点或回执写入失败仍返回 `Committed`，并带 `post_commit_io`；这不能再写成确定的 `NotPreserved`。`arrangement` 固定为 `not_arranged`，`searchability` 固定为 `not_searchable`。数据库约束也拒绝其他值。

`user_marker` 是一条已提交的观察用文本，用来确认后来的导入、失败和重试没有改掉旧值。它不是订阅事实。导入不会改写它。

## 命令与退出码

`help`、`version`、`init`、`open`、`verify`、`recover`、`hold`、`import`、`read`、`marker`、`cancel`。标准输出是 JSON。每条回执都带 `slice=T1`、`user_ready=false`、`security_complete=false`、`purchase_id=null`、`entitlement_id=null`。

| 码 | 结果 |
| --- | --- |
| 0 | 完成、已提交过、已取消、取消已挂起，或打开后就绪 |
| 2 | 用法错误 |
| 3 | 有告警且引用没有悬空，例如对象路径被拒绝但规范路径上的字节仍完整 |
| 10 | 不是库。缺失路径不会被创建 |
| 11 | 另一个进程持有写锁 |
| 12 | 需要恢复：对象缺失或哈希不符、悬空引用、无法读取的 journal、成功回执不是 JSON 对象 |
| 13 | 数据库头损坏。打开和取消都不重建该文件 |
| 14 | `user_version` 高于 1。打开和取消都不迁移、不覆盖 |
| 15 | 没有保全。包含真实 IO 错误，以及测试注入的错误 |
| 16 | 拒绝。载荷冲突、代次不符，或提取取消 |
| 17 | 请求的故障点在这次命令上不可能到达。命令不会假装成功 |
| 18 | 文件系统被识别为 NFS 或 SMB/CIFS |
| 19 | 横幅不是合成库横幅 |
| 20 | 请求的停止点没有到达 |
| 21 | 提交结果无法确定。回执带 CommandId，`new_effect` 为 null，不猜测是否已保全。这次没有能安全造出的实测 |

`--stop-at during_copy` 在对象发布前停下，包括 0 字节、1 字节、2 字节，以及把第一次 `read` 强制成 1 字节的情况。多字节读取会留下短于全文的前缀。这个强制短读只作用于路径名含 `SHORTREAD-` 的文件，不是 ext4 自然短读，也不是故障注入。一次复制只停一次，恢复后把剩余字节写完。

`--stop-at` 把检查点写到指定文件后发送 `SIGSTOP`。`--inject enospc:POINT` 或 `--inject io_error:POINT` 把见证写到指定文件后返回未保全。注入回执里的 `injected` 是 true，并写明这不是磁盘真的满了。见证文件没有出现时，注入不算已经发生。`db_commit` 注入发生在真正的 `COMMIT` 之前，事务回滚，命令行不存在；它只证明确定的回滚，不是提交结果不确定。

`cancel --kind extraction` 只返回 `extraction_cancel_not_available`。它不创建提取目录，也没有提取实现。保全命令在提交前可以取消。写者已经提交、但仍持有锁时，取消用 `mode=ro` 读取 WAL 里的命令行，返回 `AlreadyCommitted`，`cancel_applied` 为 false，`preservation` 为 `preserved`，不写 control 文件。版本检查仍用 `immutable=1`，那条连接故意看不见尚未检查点的 WAL，不能拿来判断命令是否提交，也不能用 journal 上的状态文字代替数据库。0 字节库、坏文件头和更高的 `user_version` 走与打开相同的非破坏边界。

SQLite 的 `COMMIT` 本身返回错误时，另开一条能看见 WAL 的连接：看得到命令行就是已提交，确定没有就是未保全，查询失败就是退出码 21。查询失败这条路径没有实测。

## 磁盘布局

```
SYNTHETIC_ONLY
writer.lock
vault.db
objects/obj_<id>
staging/<command-id>.partial
journal/<command-id>.json
control/<command-id>
```

`writer.lock` 的文字只说明它是 `flock` 的目标。独占、非阻塞的 `flock` 才是锁。文件里的 pid 或过期时间不能授予锁，也不能阻止锁。进程被杀死后，由内核释放锁；恢复的是锁，不是去相信残留文字。

SQLite 使用捆绑的 3.46.0，WAL，`synchronous=FULL`，`foreign_keys=ON`，`busy_timeout=0`，`user_version=1`。表是 `meta`、`objects`、`sources`、`occurrences`、`commands`。打开和取消在 `Connection::open` 之前先检查 `SQLite format 3` 文件头，避免 0 字节或坏文件被 SQLite 初始化成空库。

打开、`verify` 和 `recover` 走同一条内部路径：删除 `staging/` 里的临时文件以及 journal 的临时名，不删除 `objects/` 里的文件。journal 只有在对应命令行已经存在时才标成 committed；恢复不会根据对象文件名插入源或购买。未引用对象保持未引用，直到同一个 CommandId 带着匹配字节重试。恢复可以重复执行。

journal 是一份字段表生成的 JSON。未知键被忽略。可选字段缺失和 JSON null 相同。进行中的 journal 可以还没有 `payload_fingerprint`；这表示哈希还没记下，不是已提交结果被二次套用。手改删掉指纹属于损坏，不按普通的双应用缺陷处理。成功回执若不是 JSON 对象，重放返回退出码 12，不另建一套回执模式。

可见源的读取只走 `objects/<object_id>`。记录里的相对路径如果不是这条规范路径，会告警，但不会跟着它读到库外。

源文件读取是普通读循环，不是原子快照。单个对象上限是 64 MiB，按元数据长度拒绝，稀疏大文件不会先被读入。内容哈希是托管字节的 SHA-256，不做换行、编码或 PDF 规范化。

## 平台边界

这轮只在这台 Linux 云端虚拟机上验证。文件系统是 ext4（`findmnt` 的类型是 ext4，magic 是 `0xef53`；`stat -f` 的类型名可能显示成 ext2/ext3，以 magic 为准）。

写锁是 Unix `flock`。没有测过 Windows 原生锁。没有测过断电和坏盘。NFS magic `0x6969` 与 SMB/CIFS magic `0xFF534D42`、`0xFE534D42` 会拒绝，但本轮没有挂载这些文件系统。同步盘没有探测，也不支持。回执里的 `platform_limits` 使用这些原句，不能读成跨平台或同步盘已经通过。

## 复现

```sh
python3 synthetic/generate_corpus.py --check
cargo test --offline --test public_behavior -- --test-threads=1
scripts/run-t1-evidence.sh
```

证据脚本把环境、测试输出、JSONL、二进制哈希、许可证文本和可选的小 tmpfs 实验写到 `/opt/cursor/artifacts/subvault-t0-t1/`。测试耗时只是该次运行的起止时间，不是用户版本的性能结论。

行为测试依赖 `sha256sum`、`sqlite3`、`python3`、`cp`、`kill` 和 `gcc`。`sqlite3` 只用来篡改已经写成文档的库格式。`gcc` 只编译测试里的短读探针。短读探针、`--inject` 和真实的目录权限错误是三件不同的事：探针强制一次 1 字节读取，注入在回执里标 `injected: true`，journal 目录 `0555` 是真实的 `EACCES`，不是满盘，也不是注入。

满盘注入始终标成注入。脚本里的 tmpfs 实验如果没有免密 `sudo`，会记为跳过；它不会去写满宿主盘。宿主盘满、WAL 的 `fsync` 返回 EIO 或 ENOSPC、提交语句中途被杀，都没有测。

## 这轮没有做

整理、全文搜索、订阅规则、购买、权益、提取、导出、加密、真实数据导入、桌面壳和遥测都没有实现。T0/T1 通过不等于产品完成。
