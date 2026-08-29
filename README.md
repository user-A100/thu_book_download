# thu_book_download

清华大学教参服务平台电子书下载工具。它保留 Rust 多线程下载和本地 PDF 合成的性能，同时通过 Tampermonkey 在书籍详情页提供“一键下载”界面，不需要手动复制 token 或输入命令。

> 仅供具有平台合法访问权限的清华师生学习使用。请遵守平台规则和版权要求，不要绕过访问权限、批量传播或公开分享生成的电子书。

## 工作方式

```text
教参书籍详情页
    ↓ Tampermonkey 提交当前书籍
127.0.0.1 本地接口
    ↓
Rust 多线程下载并合成 PDF
    ↓
downloads/书籍ID.pdf
```

浏览器只负责显示操作面板和下载进度，页面下载、图像处理与 PDF 生成都由本机 Rust 程序完成。

## Windows 安装

1. 从 [Releases](../../releases) 下载最新版压缩包并解压。
2. 安装浏览器扩展 [Tampermonkey](https://www.tampermonkey.net/)。
3. 双击压缩包中的 `thubookrs.user.js`，或把文件内容复制到 Tampermonkey 的“添加新脚本”页面并保存。
4. 双击运行 `thubookrs.exe`，保持程序窗口打开。

## 使用方法

1. 在浏览器中打开并登录[清华大学教参服务平台](https://ereserves.lib.tsinghua.edu.cn/)。
2. 首次安装脚本后，建议退出平台并重新登录一次，以便脚本自动记录当前登录 token。
3. 打开一本书的详情页，地址通常为 `https://ereserves.lib.tsinghua.edu.cn/bookDetail/...`。
4. 在页面右下角设置线程数、清晰度等选项，然后点击“开始下载”。
5. 等待页面提示完成。PDF 位于 `thubookrs.exe` 同目录下的 `downloads` 文件夹。

如果页面提示“未检测到 thubookrs”，请确认 `thubookrs.exe` 窗口仍在运行。如果提示没有 token，请退出教参平台后重新登录。

## 下载选项

- 下载线程：支持 1～16，默认 4；网络不稳定时建议使用 2 或 4。
- 清晰度：支持 3～10，数值越大生成的 PDF 越清晰、体积也越大。
- 统一页面尺寸：自动将不同尺寸的页面统一为常见尺寸。
- 删除临时图片：生成 PDF 后只保留最终文件。

## 命令行兼容

无参数运行会启动浏览器服务：

```powershell
thubookrs.exe
```

也可以显式运行：

```powershell
thubookrs.exe serve
```

原项目的命令行方式仍然可用：

```powershell
thubookrs.exe "https://ereserves.lib.tsinghua.edu.cn/bookDetail/书籍ID" --token "登录token"
```

其他参数：

```text
-n <数量>         下载线程数，1～16，默认 4
-q <质量>         PDF 清晰度，3～10，默认 10
-d, --del-img     完成后删除临时图片
-r, --auto-resize 统一页面尺寸
```

## 从源码编译

安装 [Rust 工具链](https://www.rust-lang.org/tools/install) 后执行：

```powershell
git clone https://github.com/user-A100/thu_book_download.git
cd thu_book_download
cargo build --release
```

编译结果位于 `target/release/thubookrs.exe`，油猴脚本位于 `userscript/thubookrs.user.js`。

## 本地接口与安全

- 服务只监听 `127.0.0.1:19110`，不会暴露到局域网。
- 油猴脚本与本地服务自动完成随机会话密钥配对。
- 服务只接受教参平台的 HTTPS 书籍详情地址。
- 平台 token 放在本地请求体中，不写入 URL 或程序日志。
- 输出位置由 Rust 程序控制，网页不能指定任意文件路径。

## 开发与验证

```powershell
cargo fmt --check
cargo test
cargo build --release
node --check userscript/thubookrs.user.js
```

由于完整下载需要清华统一身份认证，发布前还应在具有访问权限的真实浏览器会话中完成一次端到端测试。

## 致谢

本项目基于 [Ricky1911/thubookrs](https://github.com/Ricky1911/thubookrs) 改造，原项目是对 [dylanyang17/TsinghuaBookCrawler](https://github.com/dylanyang17/TsinghuaBookCrawler) 的 Rust 重写。感谢原作者及贡献者的工作。

当前仓库主要增加了 localhost 浏览器服务、Tampermonkey 操作界面、任务进度、配对鉴权和 Windows 发布包。
