# SubVault T0 / T1

这是可丢弃的合成验证，不是可试用产品。保护方式和主要输入都还没有选定。这里的明文库只用于测试，安全设计没有完成。

格式 4 是单独的合成库，只在新建时写入。已有的格式 1、2、3 不会被迁移、改写或转换成格式 4。把旧库的 `user_version` 改成 4 仍然是不认识的较新版本，程序不会把它当成格式 4。这不是生产环境的兼容承诺。

T0 准备虚构原文和两份独立预期。T1 只证明一件事：原件字节被单独托管，并且失败时不会假装已经保全。T1 不解析订阅、不判断金额、不建立购买或权益，也不提供整理或全文搜索。正文片段命令会另记来源载明的候选；那些候选不是事实，也不是购买或账户关系。

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
- 命令以调用者给出的 CommandId 记入 SQLite。同一 CommandId 再次执行时，先核对托管对象的当前字节。对象缺失或字节被改过时返回恢复所需，`new_effect` 为 false，不复制、不覆盖、不补回旧对象。托管字节仍完整时：外部路径已经不存在，就用已经核对过的托管哈希加上这次请求的媒体类型计算指纹。媒体类型相同则返回当时的 `AlreadyCommitted`，`source_check` 为 `absent`，`new_effect` 为 false。媒体类型不同则退出码 16，`payload_conflict`，`new_effect` 为 false，不新增出现记录；已保存的媒体类型和显示名不变，托管对象仍是 `preserved`。路径还在但读不出来，或路径不是普通文件，退出码 3，outcome 是 `ExternalUnverified`，reason 分别是 `source_unreadable` 和 `source_not_a_file`。`payload_verified` 为 false，`payload_matches` 为 null，`new_effect` 为 false。这条回执的 `preservation` 是刚才核对过的那个托管对象的 `preserved`，`object_intact` 为 true。读不到字节时不比较媒体类型，也不把这次失败当成载荷冲突。路径可读且内容或媒体类型不同，返回 `payload_conflict`，`payload_matches` 为 false，`new_effect` 为 false；这是本次请求被拒绝，已经核对过的托管对象仍是 `preserved`。显示名不在指纹里，只改显示名仍是 `AlreadyCommitted`，并保留第一次的显示名。
- 回执里的 `preservation` 只有在 SQLite 提交成功并且这次调用核对过托管字节之后，才对那个源写成 `preserved`。提交成功后的 journal、检查点或回执写入失败仍返回 `Committed`，并带 `post_commit_io`；这不能再写成确定的 `NotPreserved`。`arrangement` 固定为 `not_arranged`，`searchability` 固定为 `not_searchable`。数据库约束也拒绝其他值。
- 打开、`verify` 和 `recover` 共用一条路径。命令行已经存在而 journal 标签写不回去时，不把这次失败写成整个库 `NotPreserved`。各源仍按自己的行报告 `preservation=preserved` 和 `object_intact`。标签没写成是这次调用的另一件事：退出码 3，`IntegrityAlert`，`journal_label_update` 为 `failed`，`new_effect` 为 false。重复打开的回执相同，不改对象字节，journal 文件保持原文字。同一条已提交命令再次导入仍是 `AlreadyCommitted`，回执同时带上标签没能更新；这不是 `NotPreserved`。还没有命令行的 journal 不会被改写成已提交，那条未完成命令仍是 `not_preserved`，已经提交的源仍是 `preserved`。`vault.db` 这个文件读不出来时退出码 21，reason 是 `vault_unreadable`，`new_effect` 为 null，回执里没有 `preservation`。那是这次打不开数据库文件，不是 `COMMIT` 失败，也不是断电或坏盘。
- 某一个已提交源的托管对象这次读不出来时，打开、`verify` 和 `recover` 仍列出每一个来源、旧 `user_marker`、出现记录和命令行。读得出的源是 `preservation=preserved`、`object_intact=true`。读不出的那条保留来源、对象身份、已保存的哈希、长度和媒体类型；它的 `preservation` 和 `object_intact` 是 null，表示历史提交还在、这次完整性不可核。这不是 `not_preserved`，也不是完整。告警是 `object_unreadable`，带 `during=hash_object` 和真实的 `os_error`。没有悬空引用时退出码是 3，`new_effect` 为 false。换一个对象读失败，回执里的两个来源都还在。重复打开的回执相同，对象字节不变。单独 `read` 或重放那个读不出的对象仍是退出码 12，回执不写 `preservation`。对象目录列不出来是告警 `objects_unlisted`（`during=read_objects`）；目录进不去时，来源按这次读失败处理，不写成对象缺失。未引用对象哈希读失败是告警 `orphan_unreadable`（`during=hash_orphan`），并且不给那个文件填一个哈希。这两条都不把已提交源改成未保全，也不把一个对象的未知扩大成整个库未知。journal 标签写失败仍叠在同一份报告上。
- `SYNTHETIC_ONLY` 读不出来时，打开、`verify`、`recover`、`read`、已提交命令的再次导入和 `cancel` 都是退出码 21，reason 是 `banner_unreadable`，`during` 是 `banner`，`new_effect` 为 null，回执里没有 `preservation`。对象还没被查看，所以既不写成已保全，也不写成未保全，也不列出一份假的来源清单。横幅文件不被创建或改写。横幅内容确实读到了但不匹配，仍是退出码 19，`NotSynthetic`，文件保持读到的内容。
- 还没有本命令提交行的新导入、新标记和未提交取消，失败仍是退出码 15，`preservation=not_preserved`。这只说明这次命令没有提交。
- 库已经存在，但这轮还没有读到来源行时，退出码是 21，reason 是 `vault_unseen`，`new_effect` 为 null，没有 `preservation`，也没有 `sources`。`during` 标明步骤：`lock`、`open`、`pragma`、`statfs`、`canonicalize`，或 `list_sources`、`list_objects`、`list_occurrences`、`list_commands`、`reconcile` 这种目录查询。这和横幅、`vault.db` 读失败是同一类未知：这次还没看见历史。它不表示 `COMMIT` 返回过错误。`COMMIT` 返回错误之后可见性查询也失败，才是另一条 21，reason 是 `commit_visibility_unknown`；那条没有实跑。
- 来源已经按当前规则列入打开报告之后，旁边的扫描、清理或 checkpoint 失败只追加告警，不改各源的 `preservation` 和 `object_intact`，也不编造 `incomplete_commands`。journal 目录列不出是 `journal_unlisted`；staging 目录列不出是 `staging_unlisted`；临时文件删不掉是 `journal_tmp_not_removed` 或 `staging_not_removed`；checkpoint 失败是 `checkpoint_failed`。原来是就绪时，退出码变成 3，`new_effect` 为 false。已经是 12 的缺失或损坏保持 12。
- `read` 在托管哈希已经相符之后，`--out` 写不出来是退出码 3，`preservation=preserved`，`object_intact=true`，`new_effect=false`，`during=read_copy`，`output_written=false`。副本不算成功，托管字节不改。`output_written=false` 只说明这次输出不成功。`output_cleanup` 另说清理：`not_created` 是本调用没有创建文件；`removed` 是本调用创建的半成品已经删掉；`failed` 是删除没有成功，半成品可能还在，并带 `output_cleanup_error`。
- `read --out` 在创建任何目录或文件之前，沿目标路径解析已经存在的祖先，并跟随祖先上的符号链接。判定用目录的设备号和 inode，不用路径字符串前缀。落点是当前活动库本身或库内任一目录时，退出码 16，reason 是 `output_inside_vault`，不创建文件。库内直路径、指向库内目录的父链接、以及该链接后面尚不存在的子目录，都如此；名字是普通文件还是 `obj_<32 hex>` 不改变这个拒绝。`..` 在跟随祖先链接之后再解析，所以从库内目录往上跳出后落到库外的路径可以写入，停在库内的路径仍然拒绝。无法解析落点时退出码 16，reason 是 `output_unresolved`，同样不创建。库外、名称与库目录只共享字符串前缀的目录不是库内路径。
- `read --out` 的最后一级已经是目录项，并且它所在的目录不在活动库内时，退出码 16，reason 是 `output_exists`。普通文件、目录和符号链接都算，悬空符号链接也算。不打开该目录项，不删除它，不创建或改写链接目标，也不写托管对象和外部原件。祖先已在库外、但直接父目录还不存在时，仍是 `output_parent_missing`。新建使用 Linux 的 `O_CREAT|O_EXCL|O_NOFOLLOW`。检查之后才出现的最后一级会使这次打开失败，并且不删除那个目录项；这个并发窗口没有单独跑。以上路径判定只在这台 Linux 上核对。Windows 的真实路径、大小写和 junction 没有跑，Linux 上的结果不表示这些语义已经通过。

`user_marker` 是一条已提交的观察用文本，用来确认后来的导入、失败和重试没有改掉旧值。它不是订阅事实。导入不会改写它。

## 命令与退出码

`help`、`version`、`init`、`open`、`verify`、`recover`、`hold`、`import`、`read`、`marker`、`cancel`，以及下面的 `fragment` 和 `candidates`。T1 回执带 `slice=T1`。正文命令的回执带 `slice=T2a-body`。两者都带 `user_ready=false`、`security_complete=false`、`purchase_id=null`、`entitlement_id=null`。

| 码 | 结果 |
| --- | --- |
| 0 | 完成、已提交过、已取消、取消已挂起，或打开后就绪 |
| 2 | 用法错误 |
| 3 | 有告警且引用没有悬空。对象路径被拒绝但规范路径上的字节仍完整，journal 标签没能改写，或来源报告已经列出之后扫描、清理、checkpoint 失败，都走这里。某一个托管对象、对象目录或未引用对象这次读不出，也走这里：读得出的源仍是 `preserved`，读不出的源不写保全结论。托管哈希已经相符但 `read --out` 失败同样是 3，`preservation=preserved`，`output_written=false`。`output_cleanup` 标明半成品是 `not_created`、`removed` 还是 `failed`；`output_written=false` 不等于删除成功。已提交命令的外部路径读不出或不是文件时，导入也用 3，outcome 是 `ExternalUnverified`。`init` 在目录改名成功之后，同一把已打开的父目录句柄上 `sync_all` 返回错误也走这里：outcome 仍是 `Initialized`，`created_new_vault` 和 `published` 都是 true，`parent_fsync_returned_ok` 是 false，reason 是这次错误的 `io_error` 或 `enospc`，`during` 是 `publish_dir_fsync`。`directory_durable` 是 false，意思是最终目录耐久未确认，不能单靠这个 false 倒推 sync 调用失败；调用有没有返回成功只看 `parent_fsync_returned_ok`。目录现在就在目标名上，不报未创建，也不报未保全。这次没有实跑出让 `sync_all` 返回错误：已经打开的目录 fd 在权限改掉或路径被换成另一个目录之后，`fsync` 仍返回成功。这条 `enospc` / `io_error` 映射是用构造的 `io::Error` 做的静态分类，不是观察到的 fsync 失败，也不是打开路径时的 EACCES。格式 2 和 3 在这次父目录 `fsync` 已经返回成功之后，`after_publish` 检查点写入失败也是退出码 3，outcome 仍是 `Initialized`，`published`、`created_new_vault`、`new_effect` 都是 true。reason 保持这次写入的 `io_error` 或 `enospc`，`during` 保持 `checkpoint`，并保留 `os_error` 和 `injected`，不改写成 `directory_durability_unknown`。这里的 `directory_durable=false` 是这次新澄清的「最终目录耐久未确认」，不是「父目录 fsync 返回了失败」。`parent_fsync_returned_ok=true` 只记录此前那次父目录 sync 调用实际返回成功；hook 失败不把那次成功改写成失败。这两个字段旧回执里没有定义，不能读成旧合同已经有这个语义。`output` 是请求的目标路径，只作位置提示，不重新核对身份。不报 `NotPreserved`，不写 `preservation`，也不清理。`export` 在包改名成功之后，同一把父目录句柄上 `sync_all` 返回错误同样是 3：`published` 为 true，`output_written` 仍是 true，`completeness` 不变，`parent_fsync_returned_ok` 是 false，reason 是 `io_error` 或 `enospc`，`during` 是 `export_dir_fsync`。`output` 仍是请求的 `--out` 字符串。不进入失败收尾，不删除已经改好名的包。sync 已经返回成功之后的见证或注入失败也是 3，但 `parent_fsync_returned_ok` 是 true，reason 是那次见证或注入的实际原因，不能把 `directory_durable=false` 读成 sync 失败 |
| 10 | 不是库。缺失路径不会被创建 |
| 11 | 另一个进程持有写锁 |
| 12 | 需要恢复：对象缺失或哈希不符、悬空引用、无法读取的 journal、成功回执不是 JSON 对象。单独 `read` 或重放某一个读不出的托管对象也是 12，`object_unreadable`，那条回执不写 `preservation` |
| 13 | 只读损坏。打开和取消都不重建、不覆盖。`sqlite_header` 表示文件头字节已经读到，但不是 16 字节的 `SQLite format 3\0`；读到的长度不足 16 字节，包括 0 字节文件，也是这个 reason。文件打不开，或读操作本身失败，不是 13，那是 21 的 `vault_unreadable`。同一退出码还有版本检查的只读连接打不开（`sqlite_unreadable`）、`user_version` 读失败（`user_version_unreadable`）、读到的 `user_version` 不是 1 且不高于 1（`unexpected_user_version`）、`schema_version` 那一行读失败（`meta_unreadable`）。读到的 `schema_version` 不是 `1` 是退出码 12。不认识的更高 `user_version` 是退出码 14。本程序认识的格式 2 见退出码 14。行为测试跑过坏头和 0 字节文件的打开与取消。独立复核还实跑过合法的 16 字节 `SQLite format 3\0` 头：打开返回退出码 13，reason 是 `user_version_unreadable`，文件字节保持不变。`sqlite_unreadable`、`unexpected_user_version`，以及版本检查里只读连接的 `meta_unreadable`，没有单独触发 |
| 14 | 不认识的版本。`user_version` 高于 3 是 `newer_schema`。等于 2 或 3 但主库没有配套的 `meta.vault_format`（分别是 `2` 或 `3`）且 `schema_version` 仍是 `1`，是 `vault_format_unverified`。主库头和 WAL 里的有效版本不一致时 reason 是 `format_inconsistent`。不恢复、不清理、不 checkpoint 把两边收成一个号，也不把版本号改小。这次只读核对可能创建或碰到 `-shm`。格式 1 上的正文写入口在任何新 journal 或正文 DDL 之前返回 `NeedsUpgrade`。本程序可以打开新建的格式 2 和格式 3 库。格式 3 是全新合成库，不是格式 1 或 2 的升级或降级 |
| 15 | 这次还没有提交成功，包括新导入、新标记、未提交取消、真实 IO 错误和测试注入。来源行还没有提交时，回执带 `preservation=not_preserved`。来源行已经提交之后的正文失败，包括 `object_ready` 的 journal 写失败，退出码仍可以是 15，但 `preservation` 是 `preserved`，`candidates_published` 是 false。已经核对过的历史、还没看到来源行的打开，都不用 15 |
| 16 | 拒绝。载荷冲突、代次不符，或提取取消。`read --out` 的落点在活动库内是 `output_inside_vault`；落点解析不了是 `output_unresolved`。两者都不创建目录或文件。最后一级已在库外存在时是 `output_exists`，包括普通文件、目录和符号链接；悬空符号链接不跟随，链接目标不被创建。不写托管对象、外部原件和已有输出。只在 Linux 上核对；Windows 真实路径、大小写和 junction 没有跑 |
| 17 | 请求的故障点在这次命令上不可能到达。命令不会假装成功 |
| 18 | 文件系统被识别为 NFS 或 SMB/CIFS |
| 19 | 横幅内容已经读到，并且不是合成库横幅。不改写横幅文件 |
| 20 | 请求的停止点没有到达 |
| 21 | 这次还不能下保全结论。两路都不写 `preservation`，`new_effect` 为 null。未见历史：读不到 `vault.db` 是 `vault_unreadable`，`during` 是 `sqlite_header`；读不到横幅是 `banner_unreadable`，`during` 是 `banner`；锁、数据库打开、pragma、statfs、canonicalize 和来源目录查询是 `vault_unseen`，`during` 是该步骤，不编来源清单。这些都不是 `COMMIT` 返回了错误。真正的 `COMMIT` 返回错误之后，可见性查询也失败，reason 才是 `commit_visibility_unknown`，`during` 是 `after_commit_error`。这条没有实跑。`db_commit` 注入在 `COMMIT` 之前返回，是确定回滚，不是这条。查询成功时，看得到命令行是已提交，确定没有行是退出码 15。写者仍持锁时，取消若打不开那条能看见 WAL 的连接，或命令查询失败，也是 `commit_visibility_unknown`，`during` 是 `wal_open` 或 `wal_query`；那不是 `COMMIT` 返回错误，也没有单独让它失败。横幅内容已经读到但不匹配，仍是退出码 19。标准输出的 `write` 或 `flush` 失败也是 21，并且没有 JSON 回执。进程不在打印时 panic，也不把这次退出当成 0 或 15。调用方把结果当未知，用同一个 CommandId 再查或再试，不换新 ID。这一条本身不是 `COMMIT` 返回了错误。修之前，`/dev/full` 上的短回执在 `println!` 里 panic，退出 101，走不到后面被丢掉的 `flush`，所以没有观察到退出 0。修之后短回执先留在缓冲里，失败发生在最后的 `flush`；超过这块缓冲的回执在 `write` 时失败。两条都退出 21 |

`--stop-at during_copy` 在对象发布前停下，包括 0 字节、1 字节、2 字节，以及把第一次 `read` 强制成 1 字节的情况。多字节读取会留下短于全文的前缀。这个强制短读只作用于路径名含 `SHORTREAD-` 的文件，不是 ext4 自然短读，也不是故障注入。一次复制只停一次，恢复后把剩余字节写完。

`--stop-at` 把检查点写到指定文件后发送 `SIGSTOP`。检查点文件出现只说明文件已经写完。行为测试再读到 `/proc/<pid>/status` 的 `State: T` 才发送 `SIGCONT` 或 `SIGKILL`。等待的是这个状态。只在这台 Linux 上这样验证。`init` 的 `before_dir_publish` 在构建成功、最后一次目录项诊断之后、原子发布之前，格式 1、2、3 都能到达，检查点里的 `published` 是 false。格式 1 仍然拒绝 `version_set`、`version_checkpoint`、`before_publish` 和 `after_publish`。`export` 的同名停点只在最后一次目录项核对之后、原子发布之前，检查点里的 `published` 也是 false。`before_parent_sync` 在改名成功之后、同一父目录句柄 `sync_all` 之前，格式 1、2、3 的 `init` 和 `export` 都能到达。检查点里的 `published` 是 true，并有该句柄 `fstat` 的 `parent_dev` 和 `parent_ino`，只给测试对照这次打开的目录对象，不是产品回执字段，也不在父目录被改名后跟踪新路径。`SIGKILL` 只结束进程。它不是断电，也不是硬件 fsync 已经完成的证明。`--inject enospc:POINT` 或 `--inject io_error:POINT` 把见证写到指定文件后返回未保全。注入回执里的 `injected` 是 true，并写明这不是磁盘真的满了。见证文件没有出现时，注入不算已经发生。`publish_dir_fsync` 和 `export_dir_fsync` 发生在这次 sync 已经返回成功之后，不返回未保全：退出码 3，目标目录保留，`published` 为 true，`parent_fsync_returned_ok` 为 true，reason 是 `injected_io_error` 或 `injected_enospc`，`directory_durable` 为 false。这个 false 是最终耐久未确认，不是把已经返回的成功改写成 sync 失败。`injected: true` 只说明故障是注入的，不是磁盘已经失败。格式 2 和 3 的 `after_publish` 检查点写入失败同样留在已改名的目录上，退出码 3，reason 仍是 `io_error` 或 `enospc`，`during` 仍是 `checkpoint`。这不是父目录 `fsync` 返回失败。格式 1 仍不能选择这个停点。`db_commit` 注入发生在真正的 `COMMIT` 之前，事务回滚，命令行不存在；它只证明确定的回滚，不是提交结果不确定。

`cancel --kind extraction` 只返回 `extraction_cancel_not_available`。它不创建提取目录，也没有 PDF 提取实现。`cancel --kind fragment` 只取消一次正文提交。不带 kind 或 `--kind preservation` 遇到 fragment journal 时返回 `command_kind_conflict`，不改 journal，也不写 control；锁闲、锁被占用和重复调用都一样。已经提交的命令仍是 `AlreadyCommitted`。未知 kind 在写之前返回用法错误。保全命令在提交前可以取消。写者已经提交、但仍持有锁时，取消用 `mode=ro` 读取 WAL 里的命令行，返回 `AlreadyCommitted`，`cancel_applied` 为 false，`preservation` 为 `preserved`，不写 control 文件。版本检查仍用 `immutable=1`，那条连接故意看不见尚未检查点的 WAL，不能拿来判断命令是否提交，也不能用 journal 上的状态文字代替数据库。0 字节库、坏文件头和更高的 `user_version` 走与打开相同的非破坏边界。

SQLite 的 `COMMIT` 本身返回错误时，另开一条能看见 WAL 的连接：看得到命令行就是已提交，确定没有就是未保全。查询也失败才是退出码 21，reason 是 `commit_visibility_unknown`，`during` 是 `after_commit_error`。这条没有实跑。它和 `vault_unseen` 不是一路：`vault_unseen` 只说明还没看见历史，不说明 `COMMIT` 返回过错误。

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

SQLite 使用捆绑的 3.46.0，WAL，`synchronous=FULL`，`foreign_keys=ON`，`busy_timeout=0`。`init` 默认格式 1，主库 `user_version` 是 1。`init --format 2` 只新建一个隔离库，不改已有的格式 1 库。格式 2 在临时目录里写 `user_version=2` 和 `meta.vault_format=2`，`schema_version` 仍是 `1`。格式 3 再写 `body_schema`。三种格式发布前都读回 `journal_mode`，必须是 `wal`。然后 `PRAGMA wal_checkpoint(TRUNCATE)` 核整行：捆绑的 SQLite 3.46 里，WAL 上成功的 TRUNCATE 是 busy=0、log=0、checkpointed=0；非 WAL 的成功行是 busy=0、log=-1、checkpointed=-1，不能当成全 0。`SQLITE_BUSY` 变成 busy 非 0 的一行，调用没有 Err 不算完成。写连接检查 `Connection::close` 的返回值。再用 `immutable=1` 核对这次的 `vault_id`、`user_version` 和该格式应有的 meta。格式 1 只核自己的 meta，不要求 `vault_format` 或 `body_schema`。验证连接也检查 `close`。两个连接都关闭后，目录里只能有 `SYNTHETIC_ONLY`、`writer.lock`、`vault.db` 和空的 `objects`、`staging`、`journal`、`control`；`-wal`、`-shm` 或其他名字会拒绝并留下私有目录，不手工删除。然后依次同步这三个文件、四个子目录和私有目录，最后才 `renameat2(RENAME_NOREPLACE)`。上面任一步失败都不改名，不报 published 或 Ready。`immutable=1` 看不见还留在 WAL 里的页，所以只执行 `PRAGMA user_version` 不够。表是 `meta`、`objects`、`sources`、`occurrences`、`commands`。打开和取消在 `Connection::open` 之前先检查 `SQLite format 3` 文件头，避免 0 字节或坏文件被 SQLite 初始化成空库。写连接还要先拿到 `writer.lock`，再用能看见 WAL 的只读连接核对主库头、有效版本和必要 meta。查询和忙锁取消用自己的只读快照核对，不额外占这把锁。`schema_version` 仍是 `1`，不要求它和 `user_version` 相等。

`init` 的最后一级已经是目录项时退出码 16，reason 是 `vault_path_exists`，`created_new_vault` 是 false，没有 `preservation`。普通文件、目录、已有库和符号链接都算，悬空符号链接、指回自己的链接和互相指向的链接也算。参数末尾的 `/` 仍是这一级的拼写，不另走一条跟随。不把最后一级解析成它指向的对象，不改链接，也不改它指到的文件、目录或库。中间分量不是目录、或祖先链接悬空，仍是原来的路径错误，不改报成 `vault_path_exists`。目录项核对只做友好诊断。发布用同一父目录上的 `renameat2(RENAME_NOREPLACE)`。核对之后才出现的空目录、非空目录、文件和符号链接都留在原地，退出码 16，reason 仍是 `vault_path_exists`。内核没有 `renameat2` 时退出码 18，reason 是 `rename_noreplace_unavailable`，不改走普通 `rename`，也不复制后删除。`EINVAL` 留在 `os_error` 里，不当成这个能力不可用。跨设备的 `EXDEV` 直接拒绝，不换一条跨盘复制。权限、占用和满盘各自留下原来的错误，不报成 `writer_busy`。在这台 Linux 上，`/` 和 NUL 才不能放进一个目录项；反斜杠是普通名字字节，`a\b` 是一个名字，不是 `a` 里面的 `b`。发布前会同步上面列出的文件和目录；这不证明断电、坏盘或父目录同步。格式 2 和 3 的 `after_publish` 检查点写失败发生在改名成功、并且父目录 `fsync` 已经返回成功之后。回执是退出码 3，outcome 仍是 `Initialized`，`published`、`created_new_vault`、`new_effect` 都是 true，reason 保持 `io_error` 或 `enospc`，`during` 保持 `checkpoint`。`directory_durable=false` 是这次新澄清的最终目录耐久未确认，不是把已经返回成功的 fsync 记成失败；`parent_fsync_returned_ok=true` 记录那次调用返回了成功。hook 失败不使那次成功失效。这两个字段旧回执没有定义。`output` 重复请求的目标路径，只是位置提示。不回到 `NotPreserved`，不写未创建或未提交，也不清理。发布成功后保留 `renameat2` 用的那把父目录 `File`，在这把句柄上显式 `sync_all`，然后才释放。不按原路径重新打开，也不在 `Drop` 里同步。`before_parent_sync` 在这次 sync 之前停下。父目录后来被改名时不跟踪新位置，`output` 仍只是请求路径，不声称请求路径上还是原来的目录。拿这把 fd 或改名失败仍是未发布。真实的 `sync_all` 返回错误没有做出来。同步失败回执的 `enospc` / `io_error` 映射是构造的 `io::Error`，静态分类，不是打开时的 EACCES。标准输出的 `write` 或 `flush` 仍然失败于退出码 21，并且没有 JSON，这次回执不能绕过那条边界。行为上实跑的是检查点目录权限造成的 `EACCES`，reason 是 `io_error`。`enospc` 只沿用 `io_fail` 对 `ENOSPC` 的分类，没有实跑满盘。已经建出私有目录之后如果失败，收尾不再按那条路径递归删除。路径、随机名和前缀都不是所有权。回执带 `staging_cleanup=not_attempted`、`staging_cleanup_reason` 和当初的 `staging_path`：只说明这次没有尝试按路径删除，不说明该路径上现在有没有目录，不说明位置或归属，也不指示别人按这个路径去删。目的地上的竞争者留下来。停点之后把私有目录挪走、把另一个目录放回原名、再放上目的竞争者，继续拒绝时这三处都保持原样。还没建私有目录的早拒不带这些字段。正常成功也不带。目录改名成功之后的父目录 `fsync` 失败仍是已发布，不回到这条失败收尾。这不是完整的安全清理。发布用当时打开的父目录句柄，随后在同一把句柄上 `sync_all`，不按路径重新打开。参数不是 UTF-8 时，`std::env::args` 在进入命令前 panic，退出码 101；这次不改。进程被 SIGKILL 时没有回执。只在这台 Linux 上核对。没有测过 Windows 编译、真实父目录 `fsync` 失败、断电、坏盘和 NFS。

打开、`verify` 和 `recover` 走同一条内部路径：删除 `staging/` 里的临时文件以及 journal 的临时名，不删除 `objects/` 里的文件。journal 只有在对应命令行已经存在时才尝试标成 committed。标签写失败留在 `journal_label_update=failed`，不中断这份报告，也不把已提交的源改成未保全。某一个对象读失败、对象目录列不出来或未引用对象哈希读失败，同样不中断其余来源；读不出的来源不写 `preserved` 或 `not_preserved`。恢复不会根据对象文件名插入源或购买。未引用对象保持未引用，直到同一个 CommandId 带着匹配字节重试。恢复可以重复执行。

journal 是一份字段表生成的 JSON。未知键被忽略。可选字段缺失和 JSON null 相同。进行中的 journal 可以还没有 `payload_fingerprint`；这表示哈希还没记下，不是已提交结果被二次套用。手改删掉指纹属于损坏，不按普通的双应用缺陷处理。成功回执若不是 JSON 对象，重放返回退出码 12，不另建一套回执模式。

可见源的读取只走 `objects/<object_id>`。记录里的相对路径如果不是这条规范路径，会告警，但不会跟着它读到库外。

源文件读取是普通读循环，不是原子快照。单个对象上限是 64 MiB，按元数据长度拒绝，稀疏大文件不会先被读入。内容哈希是托管字节的 SHA-256，不做换行、编码或 PDF 规范化。

## 平台边界

这轮只在这台 Linux 云端虚拟机上验证。文件系统是 ext4（`findmnt` 的类型是 ext4，magic 是 `0xef53`；`stat -f` 的类型名可能显示成 ext2/ext3，以 magic 为准）。

写锁是 Linux 上的 `flock`。一次命令持锁到返回，不是应用单实例。导出只在读取已提交内容时持锁，写包仍在锁外。编译门是 `target_os = "linux"`。不是 Linux 的目标，包括 Android 和其它 Unix，停在 `compile_error`，正式库操作不进入这份实现，也不把未接入的锁说成 Ready。`src/windows_lock.rs` 只在 `cfg(windows)` 编译：`CreateFileW` 的 `bInheritHandle` 为 FALSE，且不带 `FILE_FLAG_OVERLAPPED`；`LockFileEx` 对偏移 0、长度 1 做非阻塞独占。`VaultLock` 不调用它。`ERROR_LOCK_VIOLATION`（33）才是 Busy；`ERROR_SHARING_VIOLATION`（32）和 `ERROR_IO_PENDING`（997）仍是 Failure。`Drop` 先 `UnlockFileEx` 再 `CloseHandle`，两个返回值都不当成解锁成功。`LockFileEx` 说明进程退出或关闭仍持锁的句柄后，系统解开区域锁的时间取决于可用资源，所以不承诺立刻再次取得。这次没有做 Windows 编译、两个进程真实占用、进程终止后释放、句柄不继承。Linux 上的分类测试只核对返回值和错误码表，没有执行 `LockFileEx`。目录身份仍是 Linux `metadata` 的 device 和 inode，只用于判断输出目录是否在库内，不写入库。`src/windows_file_id.rs` 只在 `cfg(windows)` 编译：安全入口是 `BorrowedHandle<'_>`，不接受裸指针；函数内把该借用交给 `GetFileInformationByHandleEx`，类是 `FileIdInfo`（`0x12`），读取 8 字节卷序列号和 16 字节 `FILE_ID_128`。这次没有在 Windows 上编译这个签名。输出路径判断不调用它。短于或长于 16 字节的标识不是身份。失败的查询不是身份，也不表示两个文件相同。卷序列号和全部 16 字节都相同才是这份合同里的同一文件；这不表示安全路径、junction 或祖先别名已经核对，也不把路径改成小写来当身份。这次没有编译或运行这份 Windows 代码。没有测过 Windows 的真实路径、大小写折叠和 junction。没有测过断电和坏盘。NFS magic `0x6969` 与 SMB/CIFS magic `0xFF534D42`、`0xFE534D42` 会拒绝，但本轮没有挂载这些文件系统。同步盘没有探测，也不支持。回执里的 `platform_limits` 使用这些原句，不能读成跨平台或同步盘已经通过。

## 复现

```sh
python3 synthetic/generate_corpus.py --check
cargo test --offline --test public_behavior -- --test-threads=1
scripts/run-t1-evidence.sh
```

证据脚本把环境、测试输出、JSONL、二进制哈希、许可证文本和可选的小 tmpfs 实验写到 `/opt/cursor/artifacts/subvault-t0-t1/`。测试耗时只是该次运行的起止时间，不是用户版本的性能结论。

行为测试依赖 `sha256sum`、`sqlite3`、`python3`、`cp`、`kill` 和 `gcc`。`sqlite3` 只用来篡改已经写成文档的库格式。`gcc` 只编译测试里的短读探针。短读探针、`--inject` 和真实的目录权限错误是三件不同的事：探针强制一次 1 字节读取，注入在回执里标 `injected: true`，journal 目录 `0555` 是真实的 `EACCES`，不是满盘，也不是注入。提交后的导入回执仍是 `Committed`；目录保持不可写时，随后的打开是退出码 3 的告警。`vault.db` 被设成 `000` 时打开是退出码 21 的 `vault_unreadable`，不是未保全，也不是就绪。

满盘注入始终标成注入。脚本里的 tmpfs 实验如果没有免密 `sudo`，会记为跳过；它不会去写满宿主盘。宿主盘满、WAL 的 `fsync` 返回 EIO 或 ENOSPC、提交语句中途被杀，都没有测。

## 正文片段

`fragment` 和 `candidates` 的回执，包括拒绝，`slice` 都是 `T2a-body`。这不改 T1 命令的 `slice`。`fragment` 只接受调用者给出的一个文件。它不读标准输入、邮箱或剪贴板。文件必须是 UTF-8。空文件、超过 65536 字节、单行超过 1024 字节、以及超过 64 条候选，都拒绝发布候选。超长文件在读入正文之前拒绝，回执里的 `bytes_read` 是 0。接受下来的原始字节原样放进 `objects/`，不改外部文件，也不做 Unicode 规范化。

媒体类型固定为 `text/plain;charset=utf-8`，`role` 是 `user_provided_fragment`。这不是完整邮件，`eml` 是 null。定位是这段版本化字符串上的 Unicode 标量区间，外加对应的 UTF-8 字节区间。规范化名是 `identity-utf8-scalars-1`，拼接名是 `single-fragment-1`，规则名是 `stated-lines-1`，行尾名是 `lf-or-crlf-1`。这些和提取器 `subvault-body` 1 一起进入 logical key。LF 和紧挨在 LF 前的 CR 都是行尾，不进入摘录，也不改原件字节。行中间的 CR 不是行尾。

候选只记录原文里写明的句子。`Amount paid USD 15.99`、带订单说明的 `Total USD 15.99`、带贷项说明的 `Total USD 15.99` 是三种语义，金额相同也不合并。单独的 `Total`、两处 `Total` 配一条说明、或两条互相冲突的说明，都是 `unresolved_total`，`usable` 为 false。服务期和“之后没有状态”不产生观察时间，`observed_at` 保持 null，`current_status` 保持 `unknown`。两行账户邮箱和一行掩码邮箱各自保留，`attributed` 为 false，不写账户关系。认不出来的行不生成候选。

同 logical key 已经成功时，另一次提交返回 `AlreadyPublished`，不新增候选。失败和取消不写入永久失败；下一次提交使用新的 attempt。取消或晚到的发布不写入新的可用候选，已成功的候选和原件字节保留。来源行已经提交之后，这一次发布失败不会把该来源改写成 `not_preserved`；回执里的 `preservation` 是 `preserved`，`candidates_published` 是 false。状态行没有写上时，回执标明 `attempt_update` 或 `run_update` 为 `failed`，不把仍在 `running` 的 attempt 说成已经清理。来源提交之前的失败，包括 `body_schema`，仍然是 `not_preserved`。正文表由第一次成功路径上的事务添加，`meta.body_schema` 记为 `1`。这步失败会回滚，不改已有的 T1 行，也不再改 `user_version`。格式 1 仍按原来的 T1 读和保全工作。对格式 1 调用 `fragment` 或 `cancel --kind fragment`，在写任何新 journal 或正文 DDL 之前返回 `NeedsUpgrade`。格式 2 只来自新建库。journal 种类仍是 `fragment`，不改名成 `import`，也不另写一个旧程序能认出的 marker。已有格式 1 库不会被就地或自动升级。旧程序打开已发布的格式 2 库时，在加锁、写打开和清理之前返回 14。这不是成品升级或降级支持。PDF、OCR 和其它平台都没有做。格式 3 会在候选发布的同一事务里记录写明的已付金额和服务期；那是来源声明，不是已核支付。`correct` 只改写明的 USD 金额文本，`undo` 追加一条补偿决定。观察时点和当前状态仍是 unknown。

`candidates` 只读查询。它不建表，也不写库。候选的 `value_json` 或 `locator_json` 读不出来时，不返回 `value` 为 null 的候选。运行配置或 attempt 明细读不出来时同样留下告警。这些都是退出码 12，原件字节不改。

## 离线退出

`export --vault PATH --out PATH` 在现有写锁下，用只读连接读一份快照，再把包写到库外。`--out` 的落点规则和 `read --out` 相同：库内、解析不了、父目录不存在、或最后一级已经存在，都拒绝，并且不创建包。包不是活动库目录的副本，里面没有 `vault.db`。

`records.json` 有来源、出现、原件身份、来源版本、候选和定位、当前事实，以及全部决定。格式 3 和格式 4 另有 `facts.csv`，只放当前值；空的金额留空，不写成 0。`manifest.json` 记录每个文件的 SHA-256 和长度。`snapshot_id` 是 `records.json` 字节的 SHA-256，不是 SQLite 的历史版本号。原件在 `originals/<object_id>`。格式 4 的 HTML 原件只作为文本放进包里，不执行，也不自动加载其中的外部引用。保护方式仍是 `undecided`：这是合成库的明文副本，不是获准的真实资料出口。

最后一次目录项核对通过之后，才用同一父目录上的 `renameat2(RENAME_NOREPLACE)` 把临时目录发布成 `--out`。核对之后才出现的名字不会被换掉；那种失败是退出码 16 的 `output_exists`。已经建出临时目录之后的失败不再按那条路径递归删除。`output_cleanup` 是 `not_attempted`，并带 `output_cleanup_reason` 和当初的 `output_staging_path`。这只说明这次没有尝试按路径删除，不说明该路径上现在有没有目录、位置或归属，也不指示按路径删除。`output_written` 仍是 false，`completeness` 仍是 `incomplete`。目的竞争者、被挪走的原临时目录、以及放回原名的替代目录都留下来。还没建临时目录的早拒不改成 `not_attempted`，也不附一条临时路径；原来就是 `output_cleanup=not_created` 的保持这个值。正常成功不带 `output_cleanup`。发布成功后保留同一把父目录句柄，显式 `sync_all` 之后才释放，不按路径重新打开，也不在 `Drop` 里同步。内核没有这个调用，或这次调用返回跨设备错误时，不改用普通 `rename` 或复制。核对通过并且发布成功时，`completeness` 是 `complete`，退出码 0。确认缺件或哈希不符时，不带 `--allow-partial yes` 就不发布，退出码 12，`completeness` 是 `incomplete`。带了这个参数才发布 `partial`，缺件留在 `missing`，不把缺的字节补成完整。对象这次读不出来不是缺件，即使带了参数也不发布。决定链对不上、引用断了，同样不发布。写失败或 `--inject io_error:export_write` / `enospc:export_write` 仍是退出码 3，`output_written` 为 false，临时目录留在原路径，`output_cleanup` 为 `not_attempted`。已有的成功包不会被这次失败覆盖。改名成功之后，同一把句柄上的 `sync_all` 返回错误是退出码 3，`published` 为 true，`parent_fsync_returned_ok` 为 false，reason 是 `io_error` 或 `enospc`，`during` 是 `export_dir_fsync`。包留在已经改好的名字上，`output_written` 仍是 true，`completeness` 保持核对结果，不回到失败收尾，也不带 `output_cleanup`。`output` 仍是请求的 `--out`。sync 已经返回成功之后的 `--inject io_error:export_dir_fsync` / `enospc:export_dir_fsync`，或那次见证写入失败，也是退出码 3，但 `parent_fsync_returned_ok` 为 true，reason 是注入的 `injected_io_error` / `injected_enospc` 或写入的 `io_error` / `enospc`。`directory_durable=false` 是最终耐久未确认，不能倒推那次 sync 失败。不把这次写成未发布，也不删掉已经改好名的包。真实的 `sync_all` 返回错误没有做出来；错误分类是静态的。这不是完整的安全清理。SIGKILL 没有回执。

这里没有主体、购买、备份、对象租约、导出取消代次，也不选择加密。复制活动库目录仍然不算完整出口。只在这台 Linux 上核对过路径。

## 格式 4 的有限 HTML

`html --vault PATH --command-id ID --source PATH` 只接受格式 4。格式 1、2、3 在打开写会话之前拒绝，已有库不会被迁移或改写。调用者不能另给媒体类型。程序不读标准输入、邮箱或剪贴板，也不取远程资源。

接受的文件必须是非空 UTF-8，并且不超过 65536 字节。编码不猜测。拒绝发生在保全之前，原件不入库。接受之后，原始字节原样放进 `objects/`。之后的解析或提取失败仍报告 `preservation=preserved`，不把已经托管的原件说成没保住。同一 CommandId 再执行时先核对托管字节：外部文件还能读且哈希不同是 `payload_conflict`，对象不改；外部文件读不到时不把已提交对象改成 `not_preserved`。另一个 CommandId 提交同一份哈希返回 `AlreadyPreserved`，不再提取一次。一个格式 4 库只收一份 HTML。

媒体类型固定为 `text/html;charset=utf-8`，`role` 是 `user_provided_html`。这不是 `text/plain`，也不是 `user_provided_fragment`。提取器是 `subvault-html` 1，规范化 `html-static-text-1`，拼接 `html-block-1`，规则 `stated-html-blocks-1`，空白 `html-collapse-1`。偏移单位是原始 UTF-8 文件上的 Unicode 标量，不是版本化纯文本。解析用锁定的 `tl` 0.7.8 取元素和原始字节边界，实体解码用锁定的 `html-escape` 0.2.13，只解码解析器已经分开的文本节点。这两项是实现记录，不进入 logical key。

支持的块是 `p`、`div`、`li`、`h1` 到 `h6`、`blockquote`。行内容纳 `b`、`strong`、`i`、`em`、`span` 和 `br`。`html`、`body`、`ul`、`ol` 只作容器。`head`、`script`、注释，以及带 `hidden` 属性的祖先不进入文本。`br` 变成换行。ASCII 空格、制表符、CR 和换页折叠成一个空格，然后去掉两端空白；不折叠不换行空格。普通 HTML 并不都在这个范围里。块外的正文、看不见的标签（例如 `table`、`a`、`img`、`pre`）是 `out_of_scope`，不发布部分事实。任何 `style` 属性、`style` 元素，或 `rel` 含 stylesheet 的 `link`，整篇提取都是 `unsupported`：字节仍保住，程序不解释 CSS。没有对上的已付金额或服务期是 `no_match`。同一类出现不同的值是 `conflict`，冲突的那类不自动采用；另一类没有冲突时仍可单独采用。相同的重复句子采用第一条，其余标 `duplicate_statement`。

金额和服务期仍用正文里的写法：`Amount paid USD ` 加整数和两位小数，`Service period: ` 加 `YYYY-MM-DD through YYYY-MM-DD`。匹配发生在折叠后的显示行上，不按原始 HTML 的整行扫描。每条事实用包含这段行内内容的最小支持元素的一段连续原始字节。`excerpt` 是那段原始 HTML，`display_excerpt` 是解码并折叠后的文本，两者分开。字节偏移、标量偏移、来源版本和哈希都能回到原件。不另造派生对象，也不做多段双定位。来源只当文本展示：不执行，不联网，不自动浏览。

`facts` 在格式 4 上的 `slice` 是 `T4-html`。事实值仍是来源声明：`payment_verified` 为 false，`observed_at` 为 null，`current_status` 和 `time_status` 为 unknown。`correct` 和 `undo` 沿用 `t3-payload-1`，只改写明的 USD 金额文本。导出包里的 HTML 原件可能含有外部引用；`field_notes.html_original` 写明这些引用不会被自动加载。格式 1、2、3 的导出注释不增加这一条。

## 这轮没有做

整理、全文搜索、订阅规则、购买、权益、PDF 提取、OCR、加密、真实数据导入、桌面壳和遥测都没有实现。离线退出包不是备份，也不是保护方案。正文候选本身不是事实。格式 3 另有事实表，只承接写明的已付金额和服务期。格式 4 的 HTML 只做上面列出的静态文本，不是浏览器，也不表示普通 HTML 都能提取。T0/T1 通过不等于产品完成。Linux 上的结果不代表 Windows。
