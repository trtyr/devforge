//! （2026-10-26 测试分离：自 src/change/boundary.rs 内联平移，纯移动零改动。）
use super::*;

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

const REPLY_DECLARE: &str = "发现计划漏了文件。\n<route>plan</route>\n<impact-gap>\n- 文件：src/extra.rs\n- 为什么必须改：需求依赖的接口在这里\n- 证据：src/extra.rs:42\n</impact-gap>";

#[test]
fn inside_whitelist_is_clean() {
    let v = classify(
        &s(&["src/a.rs", "tests/a_test.rs"]),
        &s(&["src/a.rs", "tests/a_test.rs"]),
        "干完了\n<stage-complete>1</stage-complete>",
    );
    assert_eq!(v, BoundaryVerdict::InBoundary);
}

/// 引擎审计修复 #1 回归：白名单内改动 + route token 必须走 RoutePlan 分支回 plan，
/// 不得借「extra 为空 → InBoundary」直通 testing（跳过自检/步提交/审查三门）。
#[test]
fn route_token_with_inbound_changes_routes_back_to_plan() {
    let v = classify(
        &s(&["src/a.rs"]),
        &s(&["src/a.rs"]),
        "改着改着发现计划方案不可行\n<route>plan</route>",
    );
    assert_eq!(
        v,
        BoundaryVerdict::RoutePlan {
            gaps: Vec::new(),
            reason: "改着改着发现计划方案不可行".into()
        }
    );
}

/// 引擎审计修复 #1 回归：纯回规划请求（无改动无申报）同样是 RoutePlan 信号。
#[test]
fn route_token_with_no_changes_still_routes_back_to_plan() {
    let v = classify(
        &s(&[]),
        &s(&["src/a.rs"]),
        "计划里的验收标准无法落地\n<route>plan</route>",
    );
    assert_eq!(
        v,
        BoundaryVerdict::RoutePlan {
            gaps: Vec::new(),
            reason: "计划里的验收标准无法落地".into()
        }
    );
}

#[test]
fn out_of_boundary_without_declaration_fails() {
    let v = classify(
        &s(&["src/a.rs", "src/extra.rs"]),
        &s(&["src/a.rs"]),
        "顺手改了一下，应该没事\n<stage-complete>1</stage-complete>",
    );
    match v {
        BoundaryVerdict::OutOfBoundary { extra } => assert_eq!(extra, s(&["src/extra.rs"])),
        other => panic!("应为范围膨胀 FAIL，实际 {other:?}"),
    }
}

#[test]
fn declared_gap_routes_back_to_plan_and_whitelist_grows() {
    let mut whitelist = s(&["src/a.rs"]);
    let v = classify(&s(&["src/a.rs", "src/extra.rs"]), &whitelist, REPLY_DECLARE);
    let BoundaryVerdict::RoutePlan { gaps, .. } = v else {
        panic!("带申报应回 plan，实际 {v:?}");
    };
    assert_eq!(gaps.len(), 1);
    assert_eq!(gaps[0].file, "src/extra.rs");
    assert_eq!(gaps[0].evidence, "src/extra.rs:42");

    // 补边界后重算：越界消失
    merge_gaps_into_whitelist(&mut whitelist, &gaps);
    assert!(whitelist.contains(&"src/extra.rs".to_string()));
    let again = classify(&s(&["src/a.rs", "src/extra.rs"]), &whitelist, "干完了");
    assert_eq!(again, BoundaryVerdict::InBoundary);
}

#[test]
fn declaration_must_cover_every_extra_file() {
    let v = classify(
        &s(&["src/extra.rs", "src/another.rs"]),
        &s(&["src/a.rs"]),
        REPLY_DECLARE,
    );
    assert!(v.is_fail(), "申报只盖一半也是范围膨胀：{v:?}");
}

#[test]
fn incomplete_gap_entry_counts_as_missing_declaration() {
    let reply = "<route>plan</route>\n<impact-gap>\n- 文件：src/extra.rs\n</impact-gap>";
    let v = classify(&s(&["src/extra.rs"]), &s(&["src/a.rs"]), reply);
    assert!(v.is_fail(), "缺理由/证据的申报不算申报：{v:?}");
}

#[test]
fn route_other_than_plan_is_not_a_declaration() {
    let reply = "<route>coding</route>\n<impact-gap>\n- 文件：src/extra.rs\n- 为什么必须改：x\n- 证据：src/extra.rs:1\n</impact-gap>";
    let v = classify(&s(&["src/extra.rs"]), &s(&["src/a.rs"]), reply);
    assert!(v.is_fail(), "{v:?}");
}

#[test]
fn roundtrip_is_capped_at_three() {
    for n in 1..=MAX_ROUNDTRIPS {
        check_roundtrip(n).unwrap();
    }
    let err = check_roundtrip(MAX_ROUNDTRIPS + 1).unwrap_err();
    assert!(err.to_string().contains("上限 3"), "{err}");
    assert!(err.is_quality_failure(), "往返超限是质量失败，不该自动重启");
}

#[test]
fn replan_instruction_lists_gaps_and_asks_only_for_boundary() {
    let gaps = vec![ImpactGap {
        file: "src/extra.rs".into(),
        reason: "接口在这里".into(),
        evidence: "src/extra.rs:42".into(),
    }];
    let text = replan_instruction(&gaps, 2, "需求依赖的接口变了");
    assert!(text.contains("src/extra.rs"), "{text}");
    assert!(text.contains("只补边界"), "{text}");
    assert!(text.contains("第 2/3 次往返"), "{text}");
    assert!(text.contains("回退理由（原文）：需求依赖的接口变了"), "{text}");
}

/// 自审 major 回归：空 gaps 回规划不得空转——显式告知无缺口申报，
/// 并携带 coding 的回退理由。
#[test]
fn replan_instruction_without_gaps_carries_the_reason() {
    let text = replan_instruction(&[], 1, "计划里的验收标准无法落地");
    assert!(text.contains("无边界缺口申报"), "{text}");
    assert!(text.contains("验收标准无法落地"), "{text}");
}

/// 自审 critical 回归：被截断的 <impact-gap>（无闭合标签延伸到文末）
/// 不再抹掉 route 识别——「route=plan + 截断申报」仍走 RoutePlan。
#[test]
fn truncated_impact_gap_does_not_kill_the_route() {
    let reply = "发现计划漏了。\n<route>plan</route>\n<impact-gap>\n- 文件：src/extra.rs";
    let v = classify(&s(&["src/a.rs", "src/extra.rs"]), &s(&["src/a.rs"]), reply);
    assert!(
        matches!(v, BoundaryVerdict::RoutePlan { .. }),
        "截断申报不得判范围膨胀：{v:?}"
    );
}

/// 自审 critical 回归：协议块只认最后一个——正文里引用的示例块
/// 不得劫持解析（旧实现 text.find 取第一个块）。
#[test]
fn example_blocks_in_prose_do_not_hijack_parsing() {
    let reply = "申报格式参考：\n<impact-gap>\n- 文件：src/fake.rs\n- 为什么必须改：示例\n- 证据：src/fake.rs:1\n</impact-gap>\n\n真实申报：\n<route>plan</route>\n<impact-gap>\n- 文件：src/real.rs\n- 为什么必须改：真实缺口\n- 证据：src/real.rs:7\n</impact-gap>";
    let gaps = parse_impact_gaps(reply);
    assert_eq!(gaps.len(), 1, "只认最后一个块：{gaps:?}");
    assert_eq!(gaps[0].file, "src/real.rs");
}

/// 自审 major 回归：字段值剥包裹反引号/引号——「- 文件：`src/x.rs`」
/// 的申报不再因反引号被判范围膨胀。
#[test]
fn gap_field_values_are_sanitized() {
    let reply = "<route>plan</route>\n<impact-gap>\n- 文件：`src/extra.rs`\n- 为什么必须改：x\n- 证据：`src/extra.rs:1`\n</impact-gap>";
    let v = classify(&s(&["src/a.rs", "src/extra.rs"]), &s(&["src/a.rs"]), reply);
    let BoundaryVerdict::RoutePlan { gaps, .. } = v else {
        panic!("反引号申报应被认可，实际 {v:?}");
    };
    assert_eq!(gaps[0].file, "src/extra.rs", "反引号必须剥掉：{:?}", gaps[0]);
}

#[test]
fn route_parse_is_tolerant_of_whitespace() {
    assert_eq!(parse_route("<route> plan </route>").as_deref(), Some("plan"));
    assert_eq!(parse_route("没有路由块"), None);
}

// ---------------- 尾部协议位（验收 18-21） ----------------

#[test]
fn route_is_only_recognized_at_the_tail() {
    // 错实现形态：散文里提一句 <route> 就当回退指令（c5 场的死亡真因）→ 本用例必须变红
    assert_eq!(
        parse_route("先跑既有全套 → exit 0，15 例全绿，未触发 `<route>coding</route>`。\n<stage-complete>1</stage-complete>")
            .as_deref(),
        None,
        "散文提及 + 末尾标记 → 不是回退"
    );
    assert_eq!(
        parse_route("已声明\n<route>coding</route>").as_deref(),
        Some("coding"),
        "尾部独立行 → 是回退"
    );
    assert_eq!(
        parse_route("已声明\n<route>coding</route>\n<issues>\n- tests/a_test.rs:10 断言失败\n</issues>")
            .as_deref(),
        Some("coding"),
        "尾部 route + issues 块 → 是回退"
    );
    assert_eq!(
        parse_route("已声明\n<route>plan</route>\n<impact-gap>\n- 文件：src/extra.rs\n</impact-gap>")
            .as_deref(),
        Some("plan"),
        "尾部 route + impact-gap 块 → 是回退"
    );
}

#[test]
fn prose_after_the_protocol_block_is_not_a_tail_position() {
    // 错实现形态：协议块后面还有散文也算尾部协议位 → 本用例必须变红
    assert_eq!(
        parse_route("<route>coding</route>\n<issues>\n- x\n</issues>\n补充说明一句")
            .as_deref(),
        None,
        "协议块之后有散文 → 不是尾部协议位"
    );
}

#[test]
fn route_block_must_stand_alone_on_its_line() {
    // 错实现形态：行内提及也算 route 块 → 本用例必须变红
    assert_eq!(
        parse_route("前缀<route>coding</route>").as_deref(),
        None,
        "行内提及 → 不是 route 块"
    );
    assert_eq!(parse_route("<route></route>").as_deref(), None, "空路由值无效");
}
