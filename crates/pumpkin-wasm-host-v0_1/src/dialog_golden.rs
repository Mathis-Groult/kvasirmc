//! Golden for `protocol_dialog_from_wasm`: the plugin-side dialog turned into the protocol dialog.
//! One scene per dialog type, per body kind, per input kind (with its optional parts set and
//! unset), per button action / tooltip / width, per link label, per after-action and per
//! optional field; every text carries its own words so a swapped field shows. The record is
//! the protocol value's Debug text. Bless with `BLESS_DIALOG=1`.

use std::fmt::Write as _;

use pumpkin_util::text::TextComponent;
use pumpkin_wasm_host_common::state::PluginHostState;
use wasmtime::component::Resource;

use crate::{
    events::dialog::protocol_dialog_from_wasm,
    pumpkin::plugin::{
        item_stack::ItemStack,
        java_dialogs::{
            Action, ActionButton, AfterAction, CustomClickAction, Dialog, DialogBody, DialogInput,
            DialogInputBool, DialogInputNumberRange, DialogInputSingleOption, DialogInputText,
            DialogType, Link, LinkLabel, LinkType,
        },
        text::TextComponent as WitText,
    },
};

const GOLDEN: &str = include_str!("dialog.txt");

fn text(state: &mut PluginHostState, words: &str) -> Resource<WitText> {
    state
        .add(TextComponent::text(words.to_string()))
        .expect("text component resource")
}

fn base(state: &mut PluginHostState) -> Dialog {
    Dialog {
        title: text(state, "title"),
        type_: DialogType::Notice,
        body: Vec::new(),
        inputs: Vec::new(),
        buttons: Vec::new(),
        links: Vec::new(),
        after_action: None,
        can_close_with_escape: false,
        external_title: None,
    }
}

fn type_scenes(state: &mut PluginHostState) -> Vec<(String, Dialog)> {
    let types = [
        ("notice", DialogType::Notice),
        ("confirmation", DialogType::Confirmation),
        ("multi_action", DialogType::MultiAction),
        ("dialog_list", DialogType::DialogList),
        ("server_links", DialogType::ServerLinks),
    ];
    let mut scenes = Vec::new();
    for (name, type_) in types {
        let mut dialog = base(state);
        dialog.type_ = type_;
        scenes.push((format!("type {name}"), dialog));
    }
    scenes
}

fn flag_scenes(state: &mut PluginHostState) -> Vec<(String, Dialog)> {
    let mut scenes = Vec::new();
    for (name, after) in [
        ("none", None),
        ("peek", Some(AfterAction::Peek)),
        ("pop", Some(AfterAction::Pop)),
    ] {
        let mut dialog = base(state);
        dialog.after_action = after;
        scenes.push((format!("after_action {name}"), dialog));
    }
    for escape in [false, true] {
        let mut dialog = base(state);
        dialog.can_close_with_escape = escape;
        scenes.push((format!("can_close_with_escape {escape}"), dialog));
    }
    let mut dialog = base(state);
    dialog.external_title = Some(text(state, "external"));
    scenes.push(("external_title set".to_string(), dialog));
    scenes.push(("external_title unset".to_string(), base(state)));
    scenes
}

fn body_scenes(state: &mut PluginHostState) -> Vec<(String, Dialog)> {
    let mut scenes = Vec::new();
    let mut dialog = base(state);
    dialog.body = vec![DialogBody::PlainMessage(text(state, "message one"))];
    scenes.push(("body plain".to_string(), dialog));
    let mut dialog = base(state);
    dialog.body = vec![DialogBody::Item(Resource::<ItemStack>::new_borrow(7))];
    scenes.push(("body item".to_string(), dialog));
    let mut dialog = base(state);
    dialog.body = vec![
        DialogBody::Item(Resource::<ItemStack>::new_borrow(8)),
        DialogBody::PlainMessage(text(state, "message two")),
        DialogBody::PlainMessage(text(state, "message three")),
    ];
    scenes.push(("body item, plain, plain".to_string(), dialog));
    scenes
}

fn input_scenes(state: &mut PluginHostState) -> Vec<(String, Dialog)> {
    let mut scenes = Vec::new();
    for default_value in [false, true] {
        let mut dialog = base(state);
        dialog.inputs = vec![DialogInput::Bool(DialogInputBool {
            label: text(state, "bool label"),
            default_value,
        })];
        scenes.push((format!("input bool {default_value}"), dialog));
    }
    let mut dialog = base(state);
    dialog.inputs = vec![DialogInput::Text(DialogInputText {
        label: text(state, "text label"),
        placeholder: text(state, "text placeholder"),
        default_value: "text default".to_string(),
    })];
    scenes.push(("input text".to_string(), dialog));
    for format in [None, Some("%s of %s".to_string())] {
        let mut dialog = base(state);
        dialog.inputs = vec![DialogInput::NumberRange(DialogInputNumberRange {
            label: text(state, "range label"),
            min_value: 1.5,
            max_value: 9.25,
            initial_value: 4.75,
            step: 0.5,
            label_format: format.clone(),
        })];
        scenes.push((format!("input number_range format {format:?}"), dialog));
    }
    scenes
}

fn option_scenes(state: &mut PluginHostState) -> Vec<(String, Dialog)> {
    let mut scenes = Vec::new();
    let mut dialog = base(state);
    dialog.inputs = vec![DialogInput::SingleOption(DialogInputSingleOption {
        label: text(state, "options label"),
        options: vec![
            text(state, "option a"),
            text(state, "option b"),
            text(state, "option c"),
        ],
        initial_index: 2,
    })];
    scenes.push(("input single_option three".to_string(), dialog));
    let mut dialog = base(state);
    dialog.inputs = vec![DialogInput::SingleOption(DialogInputSingleOption {
        label: text(state, "empty options label"),
        options: Vec::new(),
        initial_index: 0,
    })];
    scenes.push(("input single_option none".to_string(), dialog));
    let mut dialog = base(state);
    dialog.inputs = vec![
        DialogInput::Bool(DialogInputBool {
            label: text(state, "first bool"),
            default_value: true,
        }),
        DialogInput::SingleOption(DialogInputSingleOption {
            label: text(state, "second options"),
            options: vec![text(state, "only option")],
            initial_index: 1,
        }),
    ];
    scenes.push(("inputs bool, single_option".to_string(), dialog));
    scenes
}

fn button(
    state: &mut PluginHostState,
    words: &str,
    tooltip: Option<&str>,
    width: Option<u32>,
    action: Action,
) -> ActionButton {
    let text = text(state, words);
    let tooltip = tooltip.map(|t| self::text(state, t));
    ActionButton {
        text,
        tooltip,
        width,
        action,
    }
}

fn button_scenes(state: &mut PluginHostState) -> Vec<(String, Dialog)> {
    let custom = |payload: Option<Vec<u8>>| {
        Action::CustomClick(CustomClickAction {
            id: "plugin:clicked".to_string(),
            payload,
        })
    };
    let open = || Action::OpenUrl("https://example.org/a".to_string());
    let cases = [
        ("open_url, tooltip, width", Some("tip"), Some(150), open()),
        ("open_url, no tooltip, no width", None, None, open()),
        (
            "custom, payload",
            Some("tip two"),
            None,
            custom(Some(vec![1, 2, 255])),
        ),
        ("custom, no payload, width", None, Some(40), custom(None)),
    ];
    let mut scenes = Vec::new();
    for (name, tooltip, width, action) in cases {
        let mut dialog = base(state);
        dialog.buttons = vec![button(state, name, tooltip, width, action)];
        scenes.push((format!("button {name}"), dialog));
    }
    let mut dialog = base(state);
    dialog.buttons = vec![
        button(state, "first", None, None, open()),
        button(state, "second", Some("second tip"), Some(9), custom(None)),
    ];
    scenes.push(("buttons two".to_string(), dialog));
    scenes
}

fn link_scenes(state: &mut PluginHostState) -> Vec<(String, Dialog)> {
    let built_in = [
        ("bug_report", LinkType::BugReport),
        ("community_guidelines", LinkType::CommunityGuidelines),
        ("support", LinkType::Support),
        ("status", LinkType::Status),
        ("feedback", LinkType::Feedback),
        ("community", LinkType::Community),
        ("website", LinkType::Website),
        ("forums", LinkType::Forums),
        ("news", LinkType::News),
        ("announcements", LinkType::Announcements),
    ];
    let mut scenes = Vec::new();
    for (name, link_type) in built_in {
        let mut dialog = base(state);
        dialog.links = vec![Link {
            label: LinkLabel::BuiltIn(link_type),
            url: format!("https://example.org/{name}"),
        }];
        scenes.push((format!("link {name}"), dialog));
    }
    let mut dialog = base(state);
    dialog.links = vec![
        Link {
            label: LinkLabel::Custom(text(state, "custom link")),
            url: "https://example.org/custom".to_string(),
        },
        Link {
            label: LinkLabel::BuiltIn(LinkType::News),
            url: "https://example.org/second".to_string(),
        },
    ];
    scenes.push(("links custom, news".to_string(), dialog));
    scenes
}

fn full_scene(state: &mut PluginHostState) -> Vec<(String, Dialog)> {
    let mut dialog = base(state);
    dialog.type_ = DialogType::MultiAction;
    dialog.body = vec![DialogBody::PlainMessage(text(state, "full message"))];
    dialog.inputs = vec![DialogInput::Text(DialogInputText {
        label: text(state, "full label"),
        placeholder: text(state, "full placeholder"),
        default_value: "full default".to_string(),
    })];
    dialog.buttons = vec![button(
        state,
        "full button",
        Some("full tip"),
        Some(60),
        Action::OpenUrl("https://example.org/full".to_string()),
    )];
    dialog.links = vec![Link {
        label: LinkLabel::BuiltIn(LinkType::Website),
        url: "https://example.org/full-link".to_string(),
    }];
    dialog.after_action = Some(AfterAction::Peek);
    dialog.can_close_with_escape = true;
    dialog.external_title = Some(text(state, "full external"));
    vec![("everything set".to_string(), dialog)]
}

fn record() -> String {
    let mut state = PluginHostState::new();
    let mut scenes = type_scenes(&mut state);
    scenes.extend(flag_scenes(&mut state));
    scenes.extend(body_scenes(&mut state));
    scenes.extend(input_scenes(&mut state));
    scenes.extend(option_scenes(&mut state));
    scenes.extend(button_scenes(&mut state));
    scenes.extend(link_scenes(&mut state));
    scenes.extend(full_scene(&mut state));
    let mut out = String::new();
    for (name, dialog) in scenes {
        let converted = protocol_dialog_from_wasm(&state, &dialog);
        writeln!(out, "== {name}\n{converted:?}").expect("write to a String");
    }
    out
}

#[test]
fn protocol_dialog_from_wasm_golden() {
    let got = record();
    if std::env::var_os("BLESS_DIALOG").is_some() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/src/dialog.txt");
        std::fs::write(path, &got).expect("write golden");
        return;
    }
    assert_eq!(
        got, GOLDEN,
        "protocol_dialog_from_wasm changed what it builds"
    );
}
