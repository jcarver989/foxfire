const TAU: f32 = 6.2831853;
const PI: f32 = 3.14159265;
/// How far in a settled frame is drawn: every rune has fallen and the fire has died down by then.
const SETTLED: f32 = 30.0;
/// The runes ride a band this far outside the composer, one per cell along it.
const BAND: f32 = 26.0;
const CELL: f32 = 24.0;
const SEED: f32 = 119.0;
/// The first rune flickers alive at WAKE_AT for FLICKER seconds; the rest wake over WAKE_FOR seconds after.
const WAKE_AT: f32 = 0.4;
const FLICKER: f32 = 0.5;
const WAKE_FOR: f32 = 2.0;
/// An ease-in exponent above 1 starts slow and accelerates, like lights coming on.
const CASCADE: f32 = 2.3;
const FALL_FOR: f32 = 0.45;
/// Beyond this distance from the band and the composer nothing is drawn.
const REACH: f32 = 64.0;

struct Palette {
    ember: vec3<f32>,
    flame: vec3<f32>,
    core: vec3<f32>,
};

/// Glows on dark canvases; on light ones the same runes and flames are drawn in the accent's ink instead.
fn effect(in: EffectInput) -> vec4<f32> {
    let age = param(0).x + param(0).y * SETTLED;
    let extent = param(1).xy * 0.5;
    let center = param(0).zw + extent;
    let radius = param(1).z;
    let palette = palette_for(param(2).xyz);
    let p = in.local;

    let edge = sd_rounded_rect(p - center, extent, min(radius, min(extent.x, extent.y)));
    let ext = max(edge, 0.0);
    let band = extent + BAND;
    let along = perimeter(p - center, band, min(radius + BAND, min(band.x, band.y)));
    if (abs(along.y) > REACH && ext > REACH) {
        return vec4<f32>(0.0);
    }
    let count = floor(along.z / CELL);
    let cell = along.z / count;
    let u = along.x / cell;
    let id = floor(u);

    let lit = wake(id, floor(count * 0.25), age);
    let falls = fall_at(id);
    let fall = ease_in((age - falls) / FALL_FOR, 1.6);
    let tilt = hash(vec2<f32>(id, SEED + 2.3));
    let size = 14.0 * (0.85 + 0.3 * tilt) * (1.0 - 0.4 * fall);
    let resting = (hash(vec2<f32>(id, SEED + 0.7)) - 0.5) * 6.0;
    let height = resting - BAND * fall;
    let x = (fract(u) - 0.5) * cell;
    let glyph = rune(rotation((tilt - 0.5) * 0.24) * vec2<f32>(x, along.y - height) / size, id + SEED) * size;
    let crumbled = step(hash(floor(p / 1.7) + id), 1.0 - fall * 1.15);
    let ghost = smoothstep(0.05, 0.5, age);
    let glow = lit.x * (0.75 + 1.2 * fall) + lit.y * 3.0 + ghost * 0.07;
    var color = (mix(palette.flame, palette.core, 0.3) * stroke(glyph, 0.75) * glow
        + palette.flame * exp(-glyph / 7.0) * lit.x * 0.05) * crumbled;

    for (var j = 0; j < 3; j++) {
        let k = f32(j);
        let flight = ease_in((age - falls - 0.04 * k) / 0.28, 2.0);
        let at = vec2<f32>((hash(vec2<f32>(id, 7.0 + k)) - 0.5) * cell * 0.7, height + BAND * flight * (1.0 - fall) - BAND * flight);
        let r = length(vec2<f32>(x, along.y) - at);
        let shown = step(0.001, flight) * (1.0 - smoothstep(0.85, 1.0, flight));
        color += (palette.core * exp(-r * r / 1.2) * 1.6 + palette.flame * exp(-r / 5.0) * 0.1) * shown;
    }

    var burning = 0.0;
    for (var j = -1; j <= 1; j++) {
        let neighbour = wrap(id + f32(j), count);
        burning = max(burning, kindled(along.x, (neighbour + 0.5) * cell, cell, along.z, fall_at(neighbour) + 0.3, age));
    }
    let settle = smoothstep(3.9, 5.1, age);
    color += rim_fire(p, center, extent, ext, burning, settle, age, palette) * smoothstep(-3.0, 1.0, edge);
    color += palette.core * stroke(ext, 2.0) * flash(age - falls - 0.3, 0.1) * 1.1;

    if (dot(param(3).xyz, vec3<f32>(0.2126, 0.7152, 0.0722)) > 0.5) {
        let ink = 1.0 - exp(-dot(color, vec3<f32>(0.6)));
        return vec4<f32>(palette.flame * 0.7 * ink, ink);
    }
    let mapped = 1.0 - exp(-color);
    return vec4<f32>(mapped, clamp(max(mapped.r, max(mapped.g, mapped.b)), 0.0, 1.0));
}

/// Ghostfire colours drawn from the theme's accent: a dim ember, a saturated flame and a near-white core.
fn palette_for(accent: vec3<f32>) -> Palette {
    let luma = dot(accent, vec3<f32>(0.2126, 0.7152, 0.0722));
    let flame = clamp(mix(vec3<f32>(luma), accent, 1.8) * 1.1, vec3<f32>(0.0), vec3<f32>(1.0));
    return Palette(flame * 0.45, flame, mix(flame, vec3<f32>(1.0), 0.72));
}

/// When rune `id` wakes: the first flickers alive, the rest follow in random order, slowly then faster,
/// each with a short stutter. Returns how lit it is and the flash as it catches.
fn wake(id: f32, first: f32, age: f32) -> vec2<f32> {
    let is_first = id == first;
    let order = select(0.04 + 0.96 * hash(vec2<f32>(id, SEED)), 0.0, is_first);
    let starts = select(reach_at(order, WAKE_AT + FLICKER, WAKE_FOR, CASCADE), WAKE_AT, is_first);
    let stutter = select(0.12 * mix(1.0, 0.3, order), FLICKER, is_first);
    return vec2<f32>(flicker_on(age, starts, stutter, id + SEED), flash(age - starts - stutter, 0.09));
}

/// Once every rune is awake they drop in a quick random patter.
fn fall_at(id: f32) -> f32 {
    return WAKE_AT + FLICKER + WAKE_FOR + 0.05 + 0.45 * hash(vec2<f32>(id, 117.0));
}

/// Fire lit where a rune landed at `land`, spreading along the edge to meet its neighbours'.
fn kindled(s: f32, centre: f32, cell: f32, around: f32, land: f32, age: f32) -> f32 {
    let apart = abs(s - centre);
    let gap = min(apart, around - apart);
    let spread = cell * (0.5 + 1.6 * clamp((age - land) / 0.5, 0.0, 1.0));
    return smoothstep(land, land + 0.08, age) * (1.0 - smoothstep(spread - 10.0, spread, gap));
}

/// Flames off the composer's edge: tall tongues on top, a low lick elsewhere, dying down as it settles.
fn rim_fire(p: vec2<f32>, center: vec2<f32>, extent: vec2<f32>, ext: f32, burning: f32, settle: f32, t: f32, palette: Palette) -> vec3<f32> {
    if (burning <= 0.0 || ext > REACH) {
        return vec3<f32>(0.0);
    }
    let noise = fbm(vec2<f32>(p.x / 20.0, (p.y + t * 55.0) / 24.0));
    let up = clamp((center.y - p.y) / extent.y, -1.0, 1.0);
    let reach = mix(5.0, 2.5, settle) + mix(30.0, 20.0, settle) * smoothstep(-0.2, 1.0, up);
    let heat = clamp((noise * 1.35 - (ext + 1.5) / reach) * burning * mix(1.15, 0.6, settle), 0.0, 1.5);
    let flames = palette.ember * smoothstep(0.02, 0.4, heat) * 0.7 + palette.flame * smoothstep(0.3, 0.75, heat) * 1.3
        + palette.core * smoothstep(0.7, 1.1, heat) * 2.6;
    let halo = palette.ember * exp(-ext / 45.0) * mix(0.1, 0.06, settle) + palette.flame * exp(-ext / 8.0) * mix(0.2, 0.12, settle);
    return flames * mix(0.7, 0.5, settle) + halo * burning;
}

/// An angular stave rune in a unit cell, y up: one or two staves with up to three twigs off them.
fn rune(uv: vec2<f32>, id: f32) -> f32 {
    let sx = select(0.0, 0.2, hash(vec2<f32>(id, 1.7)) > 0.78);
    var d = min(segment(uv, vec2<f32>(-sx, -0.42), vec2<f32>(-sx, 0.42)), segment(uv, vec2<f32>(sx, -0.42), vec2<f32>(sx, 0.42)));
    for (var k = 0; k < 3; k++) {
        let fk = f32(k);
        if (k > 0 && hash(vec2<f32>(id, fk * 2.3 + 0.1)) < 0.45) {
            continue;
        }
        let y0 = 0.42 - floor(hash(vec2<f32>(fk, id + 3.3)) * 3.0) * 0.28;
        let side = select(1.0, -1.0, hash(vec2<f32>(id + fk, 5.1)) < 0.5);
        let dy = (floor(hash(vec2<f32>(id * 1.3, fk + 9.0)) * 3.0) - 1.0) * 0.26;
        let root = vec2<f32>(side * sx, y0);
        d = min(d, segment(uv, root, root + vec2<f32>(side * 0.3, dy)));
    }
    return d;
}

/// Arc length clockwise from top-centre around a rounded rect of extent-size `b` and corner radius `r`, the
/// signed distance from its outline, and its perimeter. `p` is relative to the rect's centre.
fn perimeter(p: vec2<f32>, b: vec2<f32>, r: f32) -> vec3<f32> {
    let n = b - r;
    let corner = PI * 0.5 * r;
    let total = 4.0 * n.x + 4.0 * n.y + 4.0 * corner;
    let right = n.x + corner;
    let bottom_right = right + 2.0 * n.y;
    let bottom = bottom_right + corner;
    let bottom_left = bottom + 2.0 * n.x;
    let left = bottom_left + corner;
    let top_left = left + 2.0 * n.y;
    if (p.x > n.x && p.y < -n.y) {
        let q = p - vec2<f32>(n.x, -n.y);
        return vec3<f32>(n.x + (atan2(q.y, q.x) + PI * 0.5) * r, length(q) - r, total);
    }
    if (p.x > n.x && p.y > n.y) {
        let q = p - n;
        return vec3<f32>(bottom_right + atan2(q.y, q.x) * r, length(q) - r, total);
    }
    if (p.x < -n.x && p.y > n.y) {
        let q = p - vec2<f32>(-n.x, n.y);
        return vec3<f32>(bottom_left + (atan2(q.y, q.x) - PI * 0.5) * r, length(q) - r, total);
    }
    if (p.x < -n.x && p.y < -n.y) {
        let q = p + n;
        return vec3<f32>(top_left + (atan2(q.y, q.x) + TAU - PI) * r, length(q) - r, total);
    }
    if (p.x > n.x) {
        return vec3<f32>(right + p.y + n.y, p.x - b.x, total);
    }
    if (p.x < -n.x) {
        return vec3<f32>(left + n.y - p.y, -p.x - b.x, total);
    }
    if (p.y < 0.0) {
        return vec3<f32>(select(total + p.x, p.x, p.x >= 0.0), -p.y - b.y, total);
    }
    return vec3<f32>(bottom + n.x - p.x, p.y - b.y, total);
}

/// A tube struggling to strike: random on/off steps that hold more often as it warms, then steady.
fn flicker_on(age: f32, starts: f32, lasts: f32, seed: f32) -> f32 {
    if (age < starts) {
        return 0.0;
    }
    if (age >= starts + lasts) {
        return 1.0;
    }
    let x = (age - starts) / lasts;
    let tick = floor(age * 24.0);
    let on = step(hash(vec2<f32>(tick, seed * 1.37 + 0.5)), 0.12 + x * x * 1.05);
    return on * (0.55 + 0.45 * hash(vec2<f32>(tick + 3.1, seed)));
}

/// Rises fast and decays slowly, peaking `tau` seconds after it starts.
fn flash(x: f32, tau: f32) -> f32 {
    let y = max(x, 0.0) / tau;
    return y * exp(1.0 - y);
}

fn ease_in(x: f32, power: f32) -> f32 {
    return pow(clamp(x, 1e-6, 1.0), power);
}

/// When an ease-in front running over [start, start + lasts] reaches fraction `s` of its length.
fn reach_at(s: f32, start: f32, lasts: f32, power: f32) -> f32 {
    return start + lasts * pow(clamp(s, 1e-6, 1.0), 1.0 / power);
}

fn wrap(i: f32, n: f32) -> f32 {
    return i - n * floor(i / n);
}

fn stroke(d: f32, width: f32) -> f32 {
    return exp(-d * d / (width * width));
}

fn segment(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>) -> f32 {
    let pa = p - a;
    let ba = b - a;
    let h = clamp(dot(pa, ba) / max(dot(ba, ba), 1e-6), 0.0, 1.0);
    return length(pa - ba * h);
}

fn rotation(angle: f32) -> mat2x2<f32> {
    let c = cos(angle);
    let s = sin(angle);
    return mat2x2<f32>(c, s, -s, c);
}

fn hash(at: vec2<f32>) -> f32 {
    var p = fract(at * vec2<f32>(123.34, 456.21));
    p = p + dot(p, p + 45.32);
    return fract(p.x * p.y);
}

fn value_noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let below = mix(hash(i), hash(i + vec2<f32>(1.0, 0.0)), u.x);
    let above = mix(hash(i + vec2<f32>(0.0, 1.0)), hash(i + vec2<f32>(1.0, 1.0)), u.x);
    return mix(below, above, u.y);
}

fn fbm(at: vec2<f32>) -> f32 {
    var p = at;
    var value = 0.0;
    var amplitude = 0.5;
    let turn = mat2x2<f32>(0.8, 0.6, -0.6, 0.8);
    for (var i = 0; i < 5; i++) {
        value += amplitude * value_noise(p);
        p = turn * p * 2.02;
        amplitude *= 0.5;
    }
    return value;
}
