use super::tokens::{AVATAR_SIZE_SMALL, TINT_ALPHA};
use super::{Avatar, Hint, Tooltip, avatar, avatar_group};
use foxfire_acp::chat::Participant;
use rouge::core::{Color, Styled, Theme};

pub fn person_avatar(person: &Participant) -> Avatar {
    let theme = Theme::current();
    let (text, tint) = (person.id.clone(), person.id.clone());
    avatar(person.name.clone())
        .avatar_size(AVATAR_SIZE_SMALL)
        .text_color(move || person_color(&text, theme))
        .bg(move || person_color(&tint, theme).with_alpha(TINT_ALPHA).over(theme.get().canvas))
}

pub fn people_avatars(people: &[Participant]) -> Tooltip {
    avatar_group(people.iter().map(person_avatar)).max(4).tooltip(format_names(people))
}

/// The same color for a person wherever they are shown, legible on the theme.
pub fn person_color(id: &str, theme: Theme) -> Color {
    let canvas = theme.get().canvas;
    let slot = id.bytes().fold(0_usize, |hash, byte| hash.wrapping_mul(31).wrapping_add(byte.into()));
    let (on_dark, on_light) = SWATCHES[slot % SWATCHES.len()];
    Color::rgb(if canvas.r + canvas.g + canvas.b < 1.5 { on_dark } else { on_light })
}

pub fn format_names(people: &[Participant]) -> String {
    match people {
        [] => "people".to_owned(),
        [only] => only.name.clone(),
        [first, second] => format!("{} and {}", first.name, second.name),
        [first, second, third] => format!("{}, {}, and {}", first.name, second.name, third.name),
        [first, second, rest @ ..] => format!("{}, {}, and {} others", first.name, second.name, rest.len()),
    }
}

const SWATCHES: [(u32, u32); 8] = [
    (0x00d2_a8ff, 0x007d_3fc4),
    (0x006f_d6b5, 0x000f_7b60),
    (0x00f2_a7b8, 0x00b8_3261),
    (0x00ff_b86c, 0x009a_5500),
    (0x0079_c0ff, 0x0009_69da),
    (0x00a5_d6a7, 0x002b_7a3d),
    (0x00f6_c177, 0x008a_4b08),
    (0x00c4_a7e7, 0x0066_39ba),
];
