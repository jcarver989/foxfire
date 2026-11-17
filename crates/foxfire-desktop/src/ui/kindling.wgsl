const TAU: f32 = 6.2831853;
const PI: f32 = 3.14159265;
const SQRT3: f32 = 1.7320508;
/// How far in a settled frame is drawn: the snowflake has unfolded and the fire has died down by then.
const SETTLED: f32 = 30.0;
/// The snowflake blooms this big at the composer's centre, turning SPIN radians a second while it grows.
const SEED: f32 = 30.0;
const SPIN: f32 = 0.6;
/// Its triangle draws itself by DRAWN, then four generations of spikes grow out of it, GROW_FOR seconds each.
const DRAWN: f32 = 0.1;
const GROW_FOR: f32 = 0.085;
const BLOOMED: f32 = 0.44;
/// Then every point of it lets go at once and is drawn out onto the composer's outline.
const UNFOLD_FOR: f32 = 0.26;
/// The outline catches at SPARKS evenly spaced points and the fire runs along it at RUN pixels a second.
/// Keep CATCH_AT in step with the composer's reveal in kindling.rs.
const CATCH_AT: f32 = 0.7;
const SPARKS: f32 = 12.0;
const RUN: f32 = 900.0;
const CELL: f32 = 24.0;
/// The first flare dies down to a steady burn over this window.
const SETTLE_FROM: f32 = 1.15;
const SETTLE_TO: f32 = 2.4;
/// Beyond this distance from the composer nothing is drawn.
const REACH: f32 = 64.0;
/// A line's glow has faded out this far from it, so nothing further away is looked at.
const GLOW: f32 = 24.0;

/// Unit headings in sixths of a turn, y down; the snowflake's sides only ever point along these.
var<private> HEADINGS: array<vec2<f32>, 6> = array<vec2<f32>, 6>(
    vec2<f32>(1.0, 0.0),
    vec2<f32>(0.5, 0.8660254),
    vec2<f32>(-0.5, 0.8660254),
    vec2<f32>(-1.0, 0.0),
    vec2<f32>(-0.5, -0.8660254),
    vec2<f32>(0.5, -0.8660254),
);
/// The seed triangle's corners, pointing up, clockwise from the top.
var<private> CORNERS: array<vec2<f32>, 3> = array<vec2<f32>, 3>(
    vec2<f32>(0.0, -1.0),
    vec2<f32>(0.8660254, 0.5),
    vec2<f32>(-0.8660254, 0.5),
);

struct Palette {
    ember: vec3<f32>,
    flame: vec3<f32>,
    core: vec3<f32>,
};

/// The snowflake at one moment: `level` generations of spikes with the last grown `grow` of the way out,
/// turned by `turn`, and `unfold` of the way from its bloom onto the composer's outline.
struct Bloom {
    level: i32,
    grow: f32,
    unfold: f32,
    turn: mat2x2<f32>,
    extent: vec2<f32>,
    radius: f32,
    total: f32,
};

/// How far a pixel is from a line being drawn, and from the hot point at its drawing end.
struct Trace {
    line: f32,
    head: f32,
};

/// Glows on dark canvases; on light ones the same snowflake and flames are drawn in the accent's ink instead.
fn effect(in: EffectInput) -> vec4<f32> {
    let age = param(0).x + param(0).y * SETTLED;
    let extent = param(1).xy * 0.5;
    let center = param(0).zw + extent;
    let radius = min(param(1).z, min(extent.x, extent.y));
    let palette = palette_for(param(2).xyz);
    let p = in.local;
    let q = p - center;

    let edge = sd_rounded_rect(q, extent, radius);
    let ext = max(edge, 0.0);
    if (ext > REACH) {
        return vec4<f32>(0.0);
    }
    let settle = smoothstep(SETTLE_FROM, SETTLE_TO, age);
    var color = sigil(q, edge, extent, radius, age, settle, palette);

    let along = perimeter(q, extent, radius);
    let fire = kindled(along.x, along.z, ext, age);
    color += rim_fire(p, center, extent, ext, fire.x, settle, age, palette) * smoothstep(-3.0, 1.0, edge);
    color += palette.core * stroke(edge, 2.0) * fire.y * 1.4;

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

/// A Koch snowflake that blooms at the centre inside a small seal, then unfolds onto the outline and fades
/// into the fire it lit. `q` is relative to the composer's centre.
fn sigil(q: vec2<f32>, edge: f32, extent: vec2<f32>, radius: f32, age: f32, settle: f32, palette: Palette) -> vec3<f32> {
    let lines = 1.3 * (1.0 - 0.8 * smoothstep(0.72, 1.0, age)) * (1.0 - settle);
    if (age >= BLOOMED + UNFOLD_FOR) {
        return light(abs(edge), lines, palette);
    }
    let spin = SPIN * min(age, BLOOMED);
    let turn = rotation(spin);
    var color = vec3<f32>(0.0);

    if (age < DRAWN) {
        let drawing = drawn_triangle(q, turn * CORNERS[0] * SEED, turn * CORNERS[1] * SEED, turn * CORNERS[2] * SEED, age / DRAWN);
        color += light(drawing.line, lines, palette) + head(drawing.head, lines, palette);
    } else {
        let grown = min((age - DRAWN) / GROW_FOR, 4.0);
        let level = min(i32(floor(grown)) + 1, 4);
        let perimeter_total = 4.0 * (extent.x + extent.y - 2.0 * radius) + TAU * radius;
        let bloom = Bloom(
            level,
            smoothstep(0.0, 1.0, grown - f32(level - 1)),
            in_out(clamp((age - BLOOMED) / UNFOLD_FOR, 0.0, 1.0)),
            turn,
            extent,
            radius,
            perimeter_total,
        );
        color += light(snowflake(q, bloom), lines, palette);
        color += palette.flame * exp(-dot(q, q) / (SEED * SEED * 1.4)) * 0.16 * f32(level) / 4.0 * lines * (1.0 - bloom.unfold);
    }

    let seal = 1.1 * (1.0 - smoothstep(0.4, 0.62, age));
    if (seal > 0.0 && length(q) < SEED + GLOW) {
        let ring = drawn_ring(q, SEED * 0.55, clamp((age - 0.05) / 0.15, 0.0, 1.0));
        color += light(ring.line, seal, palette) + head(ring.head, seal, palette);
        let star = clamp((age - 0.1) / 0.15, 0.0, 1.0);
        let r = SEED * 0.5;
        for (var j = 0; j < 2; j++) {
            let a = -2.0 * spin + f32(j) * TAU / 6.0;
            let drawing = drawn_triangle(q, polar(r, a), polar(r, a + TAU / 3.0), polar(r, a + 2.0 * TAU / 3.0), star);
            color += light(drawing.line, seal * 0.8, palette) + head(drawing.head, seal * 0.8, palette);
        }
    }
    return color;
}

/// The nearest the snowflake comes to `q`, up to GLOW. Its twelve limbs, then the sixteen-point runs within
/// them, are skipped whenever they can't come nearer than the best so far.
fn snowflake(q: vec2<f32>, bloom: Bloom) -> f32 {
    let n = 3 << u32(2 * bloom.level);
    var best = GLOW;
    if (any(abs(q) > mix(vec2<f32>(SEED), bloom.extent, bloom.unfold) + GLOW)) {
        return best;
    }
    if (bloom.level < 2) {
        return min(best, trace(q, stretch(0, n, n, bloom), n, bloom));
    }
    let limb = n / 12;
    for (var l = 0; l < 12; l++) {
        let whole = stretch(l * limb, limb, n, bloom);
        if (reach(q, whole, bloom) > best) {
            continue;
        }
        if (limb == 16) {
            best = min(best, trace(q, whole, n, bloom));
            continue;
        }
        for (var first = l * limb; first < (l + 1) * limb; first += 16) {
            let run = stretch(first, 16, n, bloom);
            if (reach(q, run, bloom) <= best) {
                best = min(best, trace(q, run, n, bloom));
            }
        }
    }
    return best;
}

/// `count` segments of the snowflake from point `first`, with the stretch of outline they unfold onto.
struct Stretch {
    first: i32,
    count: i32,
    middle: vec2<f32>,
    spread: f32,
    start: vec2<f32>,
    end: vec2<f32>,
    straight: bool,
};

/// They follow one side of a Koch triangle, which stays within half its chord of the chord's middle; the
/// outline is straight unless it turns a corner.
fn stretch(first: i32, count: i32, n: i32, bloom: Bloom) -> Stretch {
    let a = koch(first, bloom.level, bloom.grow);
    let b = koch((first + count) % n, bloom.level, bloom.grow);
    let along = f32(first) / f32(n) * bloom.total;
    let start = outline_at(along, bloom.extent, bloom.radius);
    let end = outline_at(along + f32(count) / f32(n) * bloom.total, bloom.extent, bloom.radius);
    let flat = abs(start - end) < vec2<f32>(0.01) & abs(abs(start) - bloom.extent) < vec2<f32>(0.01);
    return Stretch(first, count, bloom.turn * (a + b) * 0.5 * SEED, length(b - a) * 0.5 * SEED, start, end, flat.x || flat.y);
}

/// The nearest a stretch could come to `q`: a bent stretch of outline stays within half its length of its chord.
fn reach(q: vec2<f32>, run: Stretch, bloom: Bloom) -> f32 {
    let bend = select(f32(run.count) / f32(3 << u32(2 * bloom.level)) * bloom.total * 0.5, 0.0, run.straight);
    let chord = segment(q, mix(run.middle, run.start, bloom.unfold), mix(run.middle, run.end, bloom.unfold));
    return chord - mix(run.spread, bend, bloom.unfold);
}

/// The nearest a stretch comes to `q`.
fn trace(q: vec2<f32>, run: Stretch, n: i32, bloom: Bloom) -> f32 {
    var best = GLOW;
    var start = vertex(run, 0, n, bloom);
    for (var i = 1; i <= run.count; i++) {
        let to = vertex(run, i, n, bloom);
        best = min(best, segment(q, start, to));
        start = to;
    }
    return best;
}

/// Point `i` of a stretch, where it has got to between its bloom and its place on the outline.
fn vertex(run: Stretch, i: i32, n: i32, bloom: Bloom) -> vec2<f32> {
    let k = (run.first + i) % n;
    let bloomed = bloom.turn * koch(k, bloom.level, bloom.grow) * SEED;
    if (bloom.unfold <= 0.0) {
        return bloomed;
    }
    var landed = mix(run.start, run.end, f32(i) / f32(run.count));
    if (!run.straight) {
        landed = outline_at(f32(k) / f32(n) * bloom.total, bloom.extent, bloom.radius);
    }
    return mix(bloomed, landed, bloom.unfold);
}

/// Point `i` of a unit snowflake after `level` generations, the last grown `grow` of the way out. Each side
/// of the seed triangle is split into quarters per generation, read off `i`'s base-4 digits from the top.
fn koch(i: i32, level: i32, grow: f32) -> vec2<f32> {
    let per_side = 1 << u32(2 * level);
    let side = i / per_side;
    let rest = i % per_side;
    var at = CORNERS[side];
    var heading = 1 + 2 * side;
    var size = SQRT3;
    for (var l = level - 1; l >= 0; l--) {
        let digit = (rest >> u32(2 * l)) & 3;
        size /= 3.0;
        let stride = HEADINGS[heading % 6] * size;
        if (l == 0 && grow < 1.0) {
            if (digit == 1) {
                at += stride;
            } else if (digit == 2) {
                at += stride * 1.5 + vec2<f32>(stride.y, -stride.x) * SQRT3 * 0.5 * grow;
            } else if (digit == 3) {
                at += stride * 2.0;
            }
            return at;
        }
        if (digit == 1) {
            at += stride;
            heading += 5;
        } else if (digit == 2) {
            at += stride + HEADINGS[(heading + 5) % 6] * size;
            heading += 1;
        } else if (digit == 3) {
            at += stride * 2.0;
        }
    }
    return at;
}

/// The point `s` along the composer's outline, clockwise from top-centre like `perimeter`, relative to its centre.
fn outline_at(arc: f32, b: vec2<f32>, r: f32) -> vec2<f32> {
    let n = b - r;
    let corner = PI * 0.5 * r;
    var s = wrap(arc, 4.0 * (n.x + n.y) + 4.0 * corner);
    if (s < n.x) {
        return vec2<f32>(s, -b.y);
    }
    s -= n.x;
    if (s < corner) {
        return vec2<f32>(n.x, -n.y) + polar(r, -PI * 0.5 + s / r);
    }
    s -= corner;
    if (s < 2.0 * n.y) {
        return vec2<f32>(b.x, s - n.y);
    }
    s -= 2.0 * n.y;
    if (s < corner) {
        return n + polar(r, s / r);
    }
    s -= corner;
    if (s < 2.0 * n.x) {
        return vec2<f32>(n.x - s, b.y);
    }
    s -= 2.0 * n.x;
    if (s < corner) {
        return vec2<f32>(-n.x, n.y) + polar(r, PI * 0.5 + s / r);
    }
    s -= corner;
    if (s < 2.0 * n.y) {
        return vec2<f32>(-b.x, n.y - s);
    }
    s -= 2.0 * n.y;
    if (s < corner) {
        return -n + polar(r, PI + s / r);
    }
    return vec2<f32>(s - corner - n.x, -b.y);
}

/// A triangle drawn `progress` of the way round from `a`.
fn drawn_triangle(q: vec2<f32>, a: vec2<f32>, b: vec2<f32>, c: vec2<f32>, progress: f32) -> Trace {
    var corners = array<vec2<f32>, 4>(a, b, c, a);
    var trace = Trace(GLOW, GLOW);
    for (var k = 0; k < 3; k++) {
        let f = clamp(progress * 3.0 - f32(k), 0.0, 1.0);
        if (f <= 0.0) {
            break;
        }
        let end = mix(corners[k], corners[k + 1], f);
        trace.line = min(trace.line, segment(q, corners[k], end));
        if (f < 1.0) {
            trace.head = length(q - end);
        }
    }
    return trace;
}

/// A circle drawn `progress` of the way round, clockwise from the top.
fn drawn_ring(q: vec2<f32>, r: f32, progress: f32) -> Trace {
    if (progress <= 0.0) {
        return Trace(GLOW, GLOW);
    }
    let sweep = progress * TAU;
    let end = polar(r, -PI * 0.5 + sweep);
    let turned = wrap(atan2(q.y, q.x) + PI * 0.5, TAU);
    let line = select(min(length(q - vec2<f32>(0.0, -r)), length(q - end)), abs(length(q) - r), turned <= sweep);
    return Trace(line, select(GLOW, length(q - end), progress < 1.0));
}

/// A thin bright line `d` away, with the soft bloom it throws.
fn light(d: f32, intensity: f32, palette: Palette) -> vec3<f32> {
    let bloom = palette.flame * (exp(-d / 5.0) * 0.12 + exp(-d * d / 128.0) * 0.06 + exp(-d / 14.0) * 0.035);
    return (mix(palette.flame, palette.core, 0.35) * stroke(d, 0.8) + bloom * (1.0 - smoothstep(GLOW * 0.6, GLOW, d))) * intensity;
}

/// The hot point at the end of a line still being drawn.
fn head(r: f32, intensity: f32, palette: Palette) -> vec3<f32> {
    let heat = 0.8 * min(intensity, 1.2);
    return (palette.core * exp(-r * r / 3.0) * 2.5 + palette.flame * exp(-r / 9.0) * 0.35) * heat * (1.0 - smoothstep(GLOW * 0.6, GLOW, r));
}

/// How alight the outline is at arc `s`, and the flash where it catches. Each spark catches at CATCH_AT,
/// a beat apart in threes, and burns outward until it meets its neighbours.
fn kindled(s: f32, around: f32, ext: f32, age: f32) -> vec2<f32> {
    let apart = around / SPARKS;
    let nearest = round(s / apart);
    var burning = 0.0;
    var hit = 0.0;
    for (var j = -1; j <= 1; j++) {
        let spark = wrap(nearest + f32(j), SPARKS);
        let lit = CATCH_AT + 0.015 * wrap(spark, 3.0);
        if (age < lit) {
            continue;
        }
        let away = abs(s - spark * apart);
        let gap = min(away, around - away);
        let spread = CELL * 0.55 + RUN * (age - lit);
        burning = max(burning, smoothstep(lit, lit + 0.05, age) * (1.0 - smoothstep(spread - 10.0 - ext, spread + ext * 0.3, gap)));
        hit = max(hit, flash(age - lit, 0.09) * exp(-gap * gap / (CELL * CELL * 0.8)));
    }
    return vec2<f32>(burning, hit);
}

/// Flames off the composer's edge: tall tongues on top, a low lick elsewhere, dying down as it settles.
fn rim_fire(p: vec2<f32>, center: vec2<f32>, extent: vec2<f32>, ext: f32, burning: f32, settle: f32, t: f32, palette: Palette) -> vec3<f32> {
    if (burning <= 0.0) {
        return vec3<f32>(0.0);
    }
    let noise = fbm(vec2<f32>(p.x / 20.0, (p.y + t * 55.0) / 24.0));
    let up = clamp((center.y - p.y) / extent.y, -1.0, 1.0);
    let reach = mix(5.0, 2.5, settle) + mix(30.0, 20.0, settle) * smoothstep(-0.2, 1.0, up);
    let heat = clamp((noise * 1.35 - (ext + 1.5) / reach) * burning * mix(1.15, 0.6, settle), 0.0, 1.5);
    let flames = palette.ember * smoothstep(0.02, 0.4, heat) * 0.7 + palette.flame * smoothstep(0.3, 0.75, heat) * 1.3
        + palette.core * smoothstep(0.7, 1.1, heat) * 2.6;
    let halo = palette.ember * exp(-ext / 45.0) * mix(0.1, 0.06, settle) + palette.flame * exp(-ext / 8.0) * mix(0.2, 0.12, settle);
    return flames * mix(0.7, 0.5, settle) + halo * burning * (1.0 - smoothstep(36.0, REACH, ext));
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

/// Rises fast and decays slowly, peaking `tau` seconds after it starts.
fn flash(x: f32, tau: f32) -> f32 {
    let y = max(x, 0.0) / tau;
    return y * exp(1.0 - y);
}

fn in_out(x: f32) -> f32 {
    return select(1.0 - pow(2.0 - 2.0 * x, 3.0) * 0.5, 4.0 * x * x * x, x < 0.5);
}

fn wrap(x: f32, n: f32) -> f32 {
    return x - n * floor(x / n);
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

fn polar(r: f32, angle: f32) -> vec2<f32> {
    return vec2<f32>(cos(angle), sin(angle)) * r;
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
