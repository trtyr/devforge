//! （ARC-R6 自 lint.rs 平移，纯移动。）

use super::*;

/// 检查类别（报告里逐条标注，便于路由与统计）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Check {
    /// 必填节缺失（D014 分级表）
    RequiredSection,
    /// 链接/锚点不可解析
    Link,
    /// Runbook 七件套不全
    Runbook,
    /// 实体存在性 / 同名异写（t4）
    Entity,
    /// 置信度诚实性（t8）：标 🟢 必须给得出证据
    Confidence,
    /// Mermaid 源码可解析（t9 / D005）
    Mermaid,
    /// 禁写内容（TODO/超长代码块/日志粘贴）——提示级
    Banned,
}

impl Check {
    pub fn as_str(self) -> &'static str {
        match self {
            Check::RequiredSection => "required-section",
            Check::Link => "link",
            Check::Runbook => "runbook",
            Check::Entity => "entity",
            Check::Confidence => "confidence",
            Check::Mermaid => "mermaid",
            Check::Banned => "banned",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// 直接判 FAIL（退出码非 0）
    Fail,
    /// 只提示，不影响通过
    Warn,
}

#[derive(Debug, Clone)]
pub struct Violation {
    pub file: PathBuf,
    /// 1-based 行号；跨文件的整体检查（如 Runbook 缺项）报 1
    pub line: usize,
    pub check: Check,
    pub severity: Severity,
    pub message: String,
}

impl Violation {
    pub fn render(&self) -> String {
        format!(
            "{}:{} [{}] {}",
            self.file.display(),
            self.line,
            self.check.as_str(),
            self.message
        )
    }
}

#[derive(Debug, Default)]
pub struct LintReport {
    pub(super) violations: Vec<Violation>,
}

impl LintReport {
    pub fn violations(&self) -> &[Violation] {
        &self.violations
    }

    pub fn failures(&self) -> impl Iterator<Item = &Violation> {
        self.violations
            .iter()
            .filter(|v| v.severity == Severity::Fail)
    }

    /// 0 违规才判通过（提示级不影响）。
    pub fn passed(&self) -> bool {
        self.failures().next().is_none()
    }

    pub fn summary(&self) -> String {
        let fails = self.failures().count();
        let warns = self.violations.len() - fails;
        if self.violations.is_empty() {
            return "doc-lint: 通过（0 违规）".into();
        }
        format!(
            "doc-lint: {} 个失败 / {} 个提示 → {}",
            fails,
            warns,
            if self.passed() { "通过" } else { "不通过" }
        )
    }

    pub(super) fn push(&mut self, file: &Path, line: usize, check: Check, severity: Severity, message: String) {
        self.violations.push(Violation {
            file: file.to_path_buf(),
            line,
            check,
            severity,
            message,
        });
    }

    pub(super) fn fail(&mut self, file: &Path, line: usize, check: Check, message: impl Into<String>) {
        self.push(file, line, check, Severity::Fail, message.into());
    }

    pub(super) fn warn(&mut self, file: &Path, line: usize, check: Check, message: impl Into<String>) {
        self.push(file, line, check, Severity::Warn, message.into());
    }
}

/// 条目类型（来自文档 frontmatter 的 `type:` 或清单）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryType {
    Entry,
    Function,
    Class,
    Config,
    Model,
    ErrorCode,
    /// 模块/包：一篇文档说的是一个文件或包（覆盖审计认它作「自报家门」）
    Module,
    /// 内部/私有符号：必填节只记提示，不 FAIL（D014）
    Internal,
}

impl EntryType {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "entry" | "入口" | "入口点" => Some(Self::Entry),
            "function" | "method" | "函数" => Some(Self::Function),
            "class" | "interface" | "类" => Some(Self::Class),
            "config" | "配置" | "配置项" => Some(Self::Config),
            "model" | "data" | "数据模型" => Some(Self::Model),
            "error" | "error-code" | "错误码" => Some(Self::ErrorCode),
            "module" | "package" | "模块" | "包" => Some(Self::Module),
            "internal" | "private" | "内部" => Some(Self::Internal),
            _ => None,
        }
    }

    /// D014 必填节分级表（关键词任一命中即视为该节存在）。
    pub fn required_sections(self) -> &'static [(&'static str, &'static [&'static str])] {
        match self {
            Self::Entry => &[
                ("用途", &["用途", "目的"]),
                ("输入", &["输入", "参数", "请求"]),
                ("输出", &["输出", "响应", "返回"]),
                ("错误", &["错误", "异常", "错误码"]),
                ("示例", &["示例", "用法", "example"]),
            ],
            Self::Function => &[
                ("摘要", &["摘要", "概述", "说明"]),
                ("行为语义", &["行为", "语义", "边界", "副作用"]),
                ("参数", &["参数", "args", "parameters"]),
                ("返回", &["返回", "returns"]),
                ("异常", &["异常", "错误", "raises"]),
            ],
            Self::Class => &[
                ("职责", &["职责", "范围"]),
                ("构造或初始化", &["构造", "初始化", "init"]),
                ("公开成员", &["成员", "属性", "方法"]),
                ("生命周期", &["生命周期", "何时"]),
            ],
            Self::Config => &[
                ("名称", &["名称", "名字"]),
                ("类型", &["类型", "type"]),
                ("默认值", &["默认"]),
                ("是否必填", &["必填", "可选"]),
                ("影响", &["影响", "作用"]),
            ],
            Self::Model => &[
                ("字段", &["字段", "列"]),
                ("约束", &["约束", "索引", "唯一"]),
                ("生命周期", &["生命周期", "何时"]),
                ("关系", &["关系", "关联"]),
            ],
            Self::ErrorCode => &[
                ("含义", &["含义", "说明"]),
                ("触发条件", &["触发", "条件"]),
                ("处理建议", &["处理", "客户端"]),
            ],
            Self::Internal => &[
                ("摘要", &["摘要", "概述", "说明"]),
                ("行为语义", &["行为", "语义"]),
            ],
            Self::Module => &[
                ("职责", &["职责", "作用", "定位"]),
                ("公开成员", &["公开成员", "导出", "对外接口"]),
                ("依赖", &["依赖", "引用", "上游"]),
            ],
        }
    }
}

/// Runbook 七件套（D001 文档契约）：关键词任一命中即视为该件存在。
pub(super) const RUNBOOK_SECTIONS: &[(&str, &[&str])] = &[
    ("环境要求", &["环境要求", "运行环境", "环境依赖"]),
    ("依赖安装", &["依赖安装", "安装依赖", "安装"]),
    ("配置项", &["配置项", "配置", "环境变量"]),
    ("启停与健康检查", &["启动", "停止", "健康检查", "health"]),
    ("种子数据", &["种子数据", "示例数据", "初始化数据"]),
    ("测试运行", &["测试运行", "运行测试", "测试"]),
    ("公开接口清单", &["公开接口", "接口清单", "端点", "api"]),
];
