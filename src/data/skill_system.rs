use latin1str::Latin1Str;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct BehaviorTemplate<'a> {
    #[serde(rename = "behaviorID")]
    behavior_id: i32,
    #[serde(rename = "templateID")]
    template_id: i32,
    #[serde(rename = "effectID")]
    effect_id: i32,
    #[serde(rename = "effectHandle")]
    effect_handle: &'a Latin1Str,
}

pub fn match_action_key(key: &Latin1Str) -> bool {
    matches!(
        key.as_bytes(),
        b"action"
            | b"behavior 1"
            | b"behavior 2"
            | b"miss action"
            | b"blocked action"
            | b"on_fail_blocked"
            | b"action_false"
            | b"action_true"
            | b"start_action"
            | b"behavior 3"
            | b"bahavior 2"
            | b"behavior 4"
            | b"on_success"
            | b"behavior 5"
            | b"chain_action"
            | b"behavior 0"
            | b"behavior 6"
            | b"behavior 7"
            | b"behavior 8"
            | b"on_fail_armor"
            | b"behavior"
            | b"break_action"
            | b"double_jump_action"
            | b"ground_action"
            | b"jump_action"
            | b"hit_action"
            | b"hit_action_enemy"
            | b"timeout_action"
            | b"air_action"
            | b"falling_action"
            | b"jetpack_action"
            | b"spawn_fail_action"
            | b"action_failed"
            | b"action_consumed"
            | b"blocked_action"
            | b"on_fail_immune"
            | b"moving_action"
            | b"behavior 10"
            | b"behavior 9"
    )
}
