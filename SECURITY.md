# Security Policy

## Supported versions

Popu 目前处于原型阶段，仅最新提交接受安全修复。

## Reporting

请通过仓库的私密安全报告渠道提交漏洞，不要在公开 issue 中披露可利用细节。报告应包含受影响版本、复现步骤、影响和建议缓解措施。

## Trust boundary

请求 JSON 被视为不可信数据并由 Rust 校验。外部模板目录必须由安装方显式指定，并被视为可信本地内容；v1 只读取模板 HTML/CSS，不执行模板自带 JavaScript。Popu 不提供 Shell、文件系统或网络 bridge。
