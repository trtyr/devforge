/**
 * P020-k5 反馈入口 · submit_issue（plan/coding/testing 干活 Agent 通用）
 *
 * 调用本工具 = 结构化上报。引擎从 RPC 事件流捕获 toolName=submit_issue 的 args，
 * 经 issue_sink 落库为 Open 状态的 issue（自动附 run/task 证据）；运营者分诊后
 * 才可能转任务——不要指望它立即被修复，也不要自行扩大修复范围。
 * 分发：devforge 渲染器按清单 extensions 声明拷入 profile 的 extensions/ 目录。
 */
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { Type } from "typebox";

const SubmitIssueParams = Type.Object({
	title: Type.String({
		description: "一句话标题（必填，写清现象与位置，如「编码 SOP 修复回路会把夹具改动一并回滚」）",
	}),
	body: Type.String({
		description: "正文：复现路径 / 证据（file:line、报错原文）/ 影响面。信息越全分诊越快",
	}),
	severity: Type.Union(
		[Type.Literal("info"), Type.Literal("minor"), Type.Literal("major"), Type.Literal("critical")],
		{ description: "严重度：critical=阻塞且无绕法；major=功能性受损；minor=小缺陷；info=记录性" },
	),
	category: Type.Union(
		[Type.Literal("bug"), Type.Literal("flow"), Type.Literal("model"), Type.Literal("feature")],
		{
			description:
				"类型：bug=代码缺陷；flow=流程/SOP 问题；model=模型能力/渠道问题；feature=能力缺口",
		},
	),
});

export default function (pi: ExtensionAPI) {
	pi.registerTool({
		name: "submit_issue",
		label: "Submit Issue",
		description:
			"上报问题：发现本任务目标之外的结构性问题（缺陷/流程/模型/能力缺口）时调用。上报后继续当前工作，不要停下来等修复，更不要自行去修（那属于扩大范围）。",
		parameters: SubmitIssueParams,
		async execute(_toolCallId, params, _signal, _onUpdate, _ctx) {
			return {
				content: [
					{
						type: "text",
						text: `已上报（引擎侧落库，Open 待分诊）：${params.title}。继续当前工作，不要等待修复。`,
					},
				],
			};
		},
	});
}
