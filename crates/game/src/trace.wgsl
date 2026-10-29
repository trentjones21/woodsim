// The studio's path tracer on the GPU: `trace.rs` and `grain.rs` as a compute shader, function
// for function, so the two renderers draw the same picture. `gpu.rs` puts the constants they
// share and the Sobol directions in front of this, and lays the scene out in the buffers below.
// Each run of `trace` adds a few samples to every pixel's light; `develop` turns the light into
// colors to show and save.

struct Node {
    // Its box; `low.w` is its first child or triangle and `high.w` how many triangles it holds,
    // as bits.
    low: vec4<f32>,
    high: vec4<f32>,
}

struct Tri {
    corner: vec4<f32>,
    edge1: vec4<f32>,
    edge2: vec4<f32>,
}

struct Shade {
    // `geometric.w` is the material, as bits.
    geometric: vec4<f32>,
    n0: vec4<f32>,
    n1: vec4<f32>,
    n2: vec4<f32>,
}

struct Material {
    // `kind.x` is 0 for matte, 1 for a finish over a color, 2 for wood, as bits; `kind.y` how
    // rough a finish is.
    kind: vec4<f32>,
    color: vec4<f32>,
    // What turns a point in the scene into one in the stock board: the rows of the matrix, the
    // translation in `w`.
    row0: vec4<f32>,
    row1: vec4<f32>,
    row2: vec4<f32>,
    along: vec4<f32>,
    // The log's pith in `xy` and its drift in `zw`.
    log: vec4<f32>,
    // The tree's tone in `xyz` and its seed in `w`, as bits.
    tone: vec4<f32>,
}

struct Lamp {
    center: vec4<f32>,
    u: vec4<f32>,
    v: vec4<f32>,
    radiance: vec4<f32>,
}

struct Params {
    position: vec4<f32>,
    forward: vec4<f32>,
    right: vec4<f32>,
    up: vec4<f32>,
    // The tangent of half the view's height, the aperture's radius, the focus distance, and
    // how many times wider than high the picture is.
    lens: vec4<f32>,
    // `zenith.w` is how bright the room's panels are.
    zenith: vec4<f32>,
    horizon: vec4<f32>,
    ground: vec4<f32>,
    // The picture's width and height, and the samples to take, from and up to.
    size: vec4<u32>,
    // How many lamps and triangles there are, and the first row of pixels this run takes.
    counts: vec4<u32>,
    // The exposure, and how many samples each pixel has had, for developing.
    develop: vec4<f32>,
    // The Sobol sequence's directions, 32 for each of four dimensions, four to an element.
    sobol: array<vec4<u32>, 32>,
}

@group(0) @binding(0) var<storage, read> nodes: array<Node>;
@group(0) @binding(1) var<storage, read> tris: array<Tri>;
@group(0) @binding(2) var<storage, read> shades: array<Shade>;
@group(0) @binding(3) var<storage, read> materials: array<Material>;
@group(0) @binding(4) var<storage, read> lamps: array<Lamp>;
@group(0) @binding(5) var<storage, read> params: Params;
@group(0) @binding(6) var<storage, read_write> light: array<vec4<f32>>;
@group(0) @binding(7) var<storage, read_write> pixels: array<u32>;

const PI: f32 = 3.14159265358979;
const FRAC_1_PI: f32 = 0.318309886183791;

struct Ray {
    origin: vec3<f32>,
    dir: vec3<f32>,
}

struct Hit {
    found: bool,
    t: f32,
    triangle: u32,
    u: f32,
    v: f32,
}

// A point on a surface: where, which way it faces (toward the ray), its shading frame, its
// color, and how rough its finish is (less than zero for none).
struct Surface {
    position: vec3<f32>,
    geometric: vec3<f32>,
    tangent: vec3<f32>,
    bitangent: vec3<f32>,
    normal: vec3<f32>,
    color: vec3<f32>,
    alpha: f32,
}

struct Bounce {
    ok: bool,
    wi: vec3<f32>,
    weight: vec3<f32>,
    pdf: f32,
}

// ---- Hashes and random numbers ----

fn scramble(x0: u32) -> u32 {
    var x = x0;
    x ^= x >> 16u;
    x *= 0x7feb352du;
    x ^= x >> 15u;
    x *= 0x846ca68bu;
    x ^= x >> 16u;
    return x;
}

fn unit(hash: u32) -> f32 {
    return f32(hash >> 8u) / 16777216.0;
}

fn next_random(state: ptr<function, u32>) -> f32 {
    *state = *state * 747796405u + 2891336453u;
    let s = *state;
    let word = ((s >> ((s >> 28u) + 4u)) ^ s) * 277803737u;
    return f32((word >> 22u) ^ word) * (1.0 / 4294967296.0);
}

fn sobol(index: u32, dimension: u32) -> u32 {
    var x = 0u;
    var bits = index;
    var k = 0u;
    loop {
        if bits == 0u {
            break;
        }
        if (bits & 1u) == 1u {
            let at = dimension * 32u + k;
            x ^= params.sobol[at / 4u][at % 4u];
        }
        bits >>= 1u;
        k += 1u;
    }
    return x;
}

fn owen(x0: u32, seed: u32) -> u32 {
    var x = reverseBits(x0);
    x += seed;
    x ^= x * 0x6c50b47cu;
    x ^= x * 0xb82f1e52u;
    x ^= x * 0xc7afe638u;
    x ^= x * 0x8d22f6e6u;
    return reverseBits(x);
}

fn sobol4(index: u32, seed: u32) -> vec4<f32> {
    let shuffled = owen(index, seed);
    var point = vec4<f32>(0.0);
    for (var dimension = 0u; dimension < 4u; dimension++) {
        let x = owen(
            sobol(shuffled, dimension),
            scramble(seed ^ ((dimension + 1u) * 0x9e3779b9u)),
        );
        point[dimension] = f32(x >> 8u) * (1.0 / 16777216.0);
    }
    return point;
}

// ---- Small helpers ----

fn smoothed(low: f32, high: f32, x: f32) -> f32 {
    let t = clamp((x - low) / (high - low), 0.0, 1.0);
    return t * t * (3.0 - 2.0 * t);
}

fn max_element(v: vec3<f32>) -> f32 {
    return max(max(v.x, v.y), v.z);
}

fn luminance(color: vec3<f32>) -> f32 {
    return dot(color, vec3<f32>(0.2126, 0.7152, 0.0722));
}

fn finite(v: vec3<f32>) -> bool {
    let bits = bitcast<vec3<u32>>(v) & vec3<u32>(0x7f800000u);
    return all(bits != vec3<u32>(0x7f800000u));
}

fn concentric(u: vec2<f32>) -> vec2<f32> {
    let p = u * 2.0 - vec2<f32>(1.0);
    if p.x == 0.0 && p.y == 0.0 {
        return vec2<f32>(0.0);
    }
    var r: f32;
    var theta: f32;
    if abs(p.x) > abs(p.y) {
        r = p.x;
        theta = PI / 4.0 * (p.y / p.x);
    } else {
        r = p.y;
        theta = PI / 2.0 - PI / 4.0 * (p.x / p.y);
    }
    return vec2<f32>(cos(theta), sin(theta)) * r;
}

fn cosine_hemisphere(u: vec2<f32>) -> vec3<f32> {
    let d = concentric(u);
    return vec3<f32>(d.x, d.y, sqrt(max(1.0 - dot(d, d), 0.0)));
}

fn tent(u: f32) -> f32 {
    if u < 0.5 {
        return sqrt(2.0 * u) - 1.0;
    }
    return 1.0 - sqrt(2.0 - 2.0 * u);
}

// ---- Wood ----

// One of the twelve directions to a cube's edges, dotted with `d`, as `grain::gradient` picks
// it: in fours, along x and y, x and z, then y and z, each with its signs flipped as the low two
// bits say. Picked without branching, as neighboring pixels pick differently.
fn gradient(hash: u32, d: vec3<f32>) -> f32 {
    let k = hash % 12u;
    let group = k / 4u;
    let first = select(d.x, d.y, group == 2u);
    let second = select(d.y, d.z, group >= 1u);
    return select(first, -first, (k & 1u) == 1u) + select(second, -second, (k & 2u) == 2u);
}

fn corner_noise(cell: vec3<i32>, f: vec3<f32>, offset: vec3<i32>, seed: u32) -> f32 {
    let c = bitcast<vec3<u32>>(cell + offset);
    let h = scramble(seed ^ (c.x * 0x27d4eb2du) ^ (c.y * 0x165667b1u) ^ (c.z * 0x5bd1e995u));
    return gradient(h, f - vec3<f32>(offset));
}

fn noise(p: vec3<f32>, seed: u32) -> f32 {
    let floored = floor(p);
    let f = p - floored;
    let cell = vec3<i32>(floored);
    let u = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    let x0 = mix(
        mix(corner_noise(cell, f, vec3<i32>(0, 0, 0), seed), corner_noise(cell, f, vec3<i32>(1, 0, 0), seed), u.x),
        mix(corner_noise(cell, f, vec3<i32>(0, 1, 0), seed), corner_noise(cell, f, vec3<i32>(1, 1, 0), seed), u.x),
        u.y,
    );
    let x1 = mix(
        mix(corner_noise(cell, f, vec3<i32>(0, 0, 1), seed), corner_noise(cell, f, vec3<i32>(1, 0, 1), seed), u.x),
        mix(corner_noise(cell, f, vec3<i32>(0, 1, 1), seed), corner_noise(cell, f, vec3<i32>(1, 1, 1), seed), u.x),
        u.y,
    );
    return mix(x0, x1, u.z);
}

fn fbm(p: vec3<f32>, octaves: u32, seed: u32) -> f32 {
    var sum = 0.0;
    var amplitude = 0.5;
    var scale = 1.0;
    for (var octave = 0u; octave < octaves; octave++) {
        sum += noise(p * scale, seed + octave * 7919u) * amplitude;
        amplitude *= 0.5;
        scale *= 2.0;
    }
    return sum;
}

fn wood(m: Material, at: vec3<f32>) -> vec3<f32> {
    let l = at.x;
    let w = at.y;
    let t = at.z;
    let seed = bitcast<u32>(m.tone.w);
    let q = vec2<f32>(w, t) - (m.log.xy + m.log.zw * l);
    let radius = length(q);
    let wobble = (fbm(vec3<f32>(q.x * 12.0, q.y * 12.0, l * 0.8), 3u, seed)
        + 0.5 * noise(vec3<f32>(q.x * 30.0, q.y * 30.0, l * 4.0), seed + 7u))
        * RING * WAVINESS;
    let ripple = noise(vec3<f32>(q.x * 90.0, q.y * 90.0, l * 9.0), seed + 1u) * RING * 0.15;
    let years = noise(vec3<f32>(radius * 70.0, 0.5, 0.5), seed + 8u) * RING * 2.5;
    let ring = (radius + wobble + ripple + years) / RING;
    let index = bitcast<u32>(i32(floor(ring)));
    let into = ring - floor(ring);
    let start = 0.52 + 0.34 * unit(scramble(index ^ (seed + 2u)));
    let fade = unit(scramble(index ^ (seed + 3u)));
    let strength = 1.0 - 0.75 * fade * fade;
    let latewood = smoothed(start, start + 0.13, into) * (1.0 - smoothed(0.985, 1.0, into)) * strength;
    let early = EARLYWOOD * (1.0 - 0.1 * smoothed(0.1, start, into));
    let color = mix(early, LATEWOOD, latewood);
    let fiber = noise(vec3<f32>(w * 2200.0, t * 2200.0, l * 24.0), seed + 4u);
    let pore = noise(vec3<f32>(w * 5200.0, t * 5200.0, l * 300.0), seed + 6u);
    let blotch = fbm(vec3<f32>(l * 1.4, w * 5.0, t * 5.0), 2u, seed + 5u);
    return color * m.tone.xyz * (1.0 + 0.07 * fiber + 0.04 * pore + 0.12 * blotch);
}

// ---- The scene ----

fn hit_triangle(index: u32, ray: Ray, limit: f32) -> vec3<f32> {
    // How far, and the weights of the second and third corners; a negative distance for a miss.
    let tri = tris[index];
    let edge1 = tri.edge1.xyz;
    let edge2 = tri.edge2.xyz;
    let p = cross(ray.dir, edge2);
    let det = dot(edge1, p);
    if abs(det) < 1e-14 {
        return vec3<f32>(-1.0, 0.0, 0.0);
    }
    let inverse = 1.0 / det;
    let s = ray.origin - tri.corner.xyz;
    let u = dot(s, p) * inverse;
    if u < 0.0 || u > 1.0 {
        return vec3<f32>(-1.0, 0.0, 0.0);
    }
    let q = cross(s, edge1);
    let v = dot(ray.dir, q) * inverse;
    if v < 0.0 || u + v > 1.0 {
        return vec3<f32>(-1.0, 0.0, 0.0);
    }
    let t = dot(edge2, q) * inverse;
    if t > 0.0 && t < limit {
        return vec3<f32>(t, u, v);
    }
    return vec3<f32>(-1.0, 0.0, 0.0);
}

fn entered(node: Node, origin: vec3<f32>, inverse: vec3<f32>, limit: f32) -> f32 {
    // How far along the ray it enters the box, or a negative number if it doesn't.
    let t0 = (node.low.xyz - origin) * inverse;
    let t1 = (node.high.xyz - origin) * inverse;
    let near = max(max_element(min(t0, t1)), 0.0);
    let lows = max(t0, t1);
    let far = min(min(min(lows.x, lows.y), lows.z), limit);
    if near <= far {
        return near;
    }
    return -1.0;
}

// The nearest triangle the ray meets before `t_max`, or, if `first_only`, whether it meets one
// at all.
fn walk(ray: Ray, t_max: f32, first_only: bool) -> Hit {
    var best = Hit(false, t_max, 0u, 0.0, 0.0);
    if params.counts.y == 0u {
        return best;
    }
    let safe = select(ray.dir, vec3<f32>(1e-30), abs(ray.dir) < vec3<f32>(1e-30));
    let inverse = 1.0 / safe;
    var stack: array<u32, 32>;
    var depth = 1u;
    stack[0] = 0u;
    loop {
        if depth == 0u {
            break;
        }
        depth -= 1u;
        let node = nodes[stack[depth]];
        let count = bitcast<u32>(node.high.w);
        let start_at = bitcast<u32>(node.low.w);
        if count > 0u {
            for (var k = start_at; k < start_at + count; k++) {
                let found = hit_triangle(k, ray, best.t);
                if found.x > 0.0 {
                    best = Hit(true, found.x, k, found.y, found.z);
                    if first_only {
                        return best;
                    }
                }
            }
            continue;
        }
        let a = start_at;
        let b = start_at + 1u;
        let near_a = entered(nodes[a], ray.origin, inverse, best.t);
        let near_b = entered(nodes[b], ray.origin, inverse, best.t);
        if near_a >= 0.0 && near_b >= 0.0 {
            if near_a <= near_b {
                stack[depth] = b;
                stack[depth + 1u] = a;
            } else {
                stack[depth] = a;
                stack[depth + 1u] = b;
            }
            depth += 2u;
        } else if near_a >= 0.0 {
            stack[depth] = a;
            depth += 1u;
        } else if near_b >= 0.0 {
            stack[depth] = b;
            depth += 1u;
        }
    }
    return best;
}

fn lamp_normal(lamp: Lamp) -> vec3<f32> {
    return normalize(cross(lamp.u.xyz, lamp.v.xyz));
}

fn lamp_area(lamp: Lamp) -> f32 {
    return 4.0 * length(cross(lamp.u.xyz, lamp.v.xyz));
}

// How far along the ray it meets the lamp's face from in front, and the cosine there; a
// negative distance for a miss.
fn hit_lamp(lamp: Lamp, ray: Ray, t_max: f32) -> vec2<f32> {
    let normal = lamp_normal(lamp);
    let facing = -dot(ray.dir, normal);
    if facing <= 1e-6 {
        return vec2<f32>(-1.0, 0.0);
    }
    let t = dot(ray.origin - lamp.center.xyz, normal) / facing;
    if t <= 0.0 || t >= t_max {
        return vec2<f32>(-1.0, 0.0);
    }
    let offset = ray.origin + ray.dir * t - lamp.center.xyz;
    let u = lamp.u.xyz;
    let v = lamp.v.xyz;
    if abs(dot(offset, u)) <= dot(u, u) && abs(dot(offset, v)) <= dot(v, v) {
        return vec2<f32>(t, facing);
    }
    return vec2<f32>(-1.0, 0.0);
}

fn environment(dir: vec3<f32>) -> vec3<f32> {
    let up = dir.y;
    var color: vec3<f32>;
    if up >= 0.0 {
        color = mix(params.horizon.xyz, params.zenith.xyz, sqrt(up));
    } else {
        color = mix(params.horizon.xyz, params.ground.xyz, min(-up * 4.0, 1.0));
    }
    let back = -dir.z;
    let panel = smoothed(0.8, 0.86, abs(dir.x)) * smoothed(0.85, 0.6, abs(dir.y))
        * smoothed(-0.55, -0.45, back) * smoothed(0.55, 0.45, back);
    let overhead = smoothed(0.9, 0.95, up);
    let panels = params.zenith.w;
    return color + vec3<f32>(panels * panel + panels * 0.6 * overhead);
}

fn camera_ray(at: vec2<f32>, lens: vec2<f32>) -> Ray {
    let tan_half = params.lens.x;
    let aspect = params.lens.w;
    let forward = params.forward.xyz;
    let right = params.right.xyz;
    let up = params.up.xyz;
    let toward = normalize(forward + right * (at.x * tan_half * aspect) + up * (at.y * tan_half));
    let focused = params.position.xyz + toward * (params.lens.z / dot(toward, forward));
    let disc = concentric(lens) * params.lens.y;
    let origin = params.position.xyz + right * disc.x + up * disc.y;
    return Ray(origin, normalize(focused - origin));
}

// ---- Reflection ----

fn schlick(cosine: f32) -> f32 {
    let m = clamp(1.0 - cosine, 0.0, 1.0);
    let m2 = m * m;
    return FINISH_REFLECTANCE + (1.0 - FINISH_REFLECTANCE) * m2 * m2 * m;
}

fn ggx(cos_h: f32, a2: f32) -> f32 {
    let k = cos_h * cos_h * (a2 - 1.0) + 1.0;
    return a2 / (PI * k * k);
}

fn smith_g1(cosine: f32, a2: f32) -> f32 {
    return 2.0 * cosine / (cosine + sqrt(a2 + (1.0 - a2) * cosine * cosine));
}

fn sample_visible_normal(o: vec3<f32>, alpha: f32, u: vec2<f32>) -> vec3<f32> {
    let v = normalize(vec3<f32>(alpha * o.x, alpha * o.y, o.z));
    let along = v.x * v.x + v.y * v.y;
    var t1 = vec3<f32>(1.0, 0.0, 0.0);
    if along > 0.0 {
        t1 = vec3<f32>(-v.y, v.x, 0.0) / sqrt(along);
    }
    let t2 = cross(v, t1);
    let r = sqrt(u.x);
    let phi = 2.0 * PI * u.y;
    let p1 = r * cos(phi);
    let s = 0.5 * (1.0 + v.z);
    let p2 = (1.0 - s) * sqrt(max(1.0 - p1 * p1, 0.0)) + s * r * sin(phi);
    let n = t1 * p1 + t2 * p2 + v * sqrt(max(1.0 - p1 * p1 - p2 * p2, 0.0));
    return normalize(vec3<f32>(alpha * n.x, alpha * n.y, max(n.z, 0.0)));
}

fn to_local(s: Surface, v: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(dot(v, s.tangent), dot(v, s.bitangent), dot(v, s.normal));
}

fn to_world(s: Surface, v: vec3<f32>) -> vec3<f32> {
    return s.tangent * v.x + s.bitangent * v.y + s.normal * v.z;
}

fn leaving(s: Surface, dir: vec3<f32>) -> Ray {
    var side = -1.0;
    if dot(dir, s.geometric) >= 0.0 {
        side = 1.0;
    }
    return Ray(s.position + s.geometric * (OFFSET * side), dir);
}

fn glossy_share(s: Surface, cos_o: f32) -> f32 {
    if s.alpha < 0.0 {
        return 0.0;
    }
    let fresnel = schlick(cos_o);
    let diffuse = (1.0 - fresnel) * luminance(s.color);
    return clamp(fresnel / (fresnel + diffuse), 0.15, 0.9);
}

// The light reflected toward `wo` from `wi` (times the cosine at `wi`) in `xyz`, and how likely
// `sample_bounce` was to pick `wi` in `w`.
fn eval(s: Surface, wo: vec3<f32>, wi: vec3<f32>) -> vec4<f32> {
    let o = to_local(s, wo);
    let i = to_local(s, wi);
    if o.z <= 0.0 || i.z <= 0.0 {
        return vec4<f32>(0.0);
    }
    let diffuse_pdf = i.z * FRAC_1_PI;
    if s.alpha < 0.0 {
        return vec4<f32>(s.color * FRAC_1_PI * i.z, diffuse_pdf);
    }
    let a2 = s.alpha * s.alpha;
    let h = normalize(o + i);
    let d = ggx(h.z, a2);
    let g1_o = smith_g1(o.z, a2);
    let fresnel = schlick(dot(o, h));
    let specular = d * g1_o * smith_g1(i.z, a2) * fresnel / (4.0 * o.z);
    let diffuse = s.color * FRAC_1_PI * (1.0 - schlick(o.z)) * (1.0 - schlick(i.z)) * i.z * 1.05;
    let share = glossy_share(s, o.z);
    let pdf = share * d * g1_o / (4.0 * o.z) + (1.0 - share) * diffuse_pdf;
    return vec4<f32>(diffuse + vec3<f32>(specular), pdf);
}

fn sample_bounce(s: Surface, wo: vec3<f32>, u: vec2<f32>, choose: f32) -> Bounce {
    let none = Bounce(false, vec3<f32>(0.0), vec3<f32>(0.0), 0.0);
    let o = to_local(s, wo);
    if o.z <= 0.0 {
        return none;
    }
    var i: vec3<f32>;
    if s.alpha >= 0.0 && choose < glossy_share(s, o.z) {
        let h = sample_visible_normal(o, s.alpha, u);
        i = h * (2.0 * dot(o, h)) - o;
        if i.z <= 0.0 {
            return none;
        }
    } else {
        i = cosine_hemisphere(u);
    }
    let wi = to_world(s, i);
    if dot(wi, s.geometric) <= 0.0 {
        return none;
    }
    let f = eval(s, wo, wi);
    if f.w <= 0.0 {
        return none;
    }
    return Bounce(true, wi, f.xyz / f.w, f.w);
}

fn surface_at(hit: Hit, ray: Ray) -> Surface {
    let shade = shades[hit.triangle];
    let position = ray.origin + ray.dir * hit.t;
    var geometric = shade.geometric.xyz;
    var normal = normalize(shade.n0.xyz * (1.0 - hit.u - hit.v) + shade.n1.xyz * hit.u + shade.n2.xyz * hit.v);
    if dot(geometric, ray.dir) > 0.0 {
        geometric = -geometric;
    }
    if dot(normal, geometric) < 0.0 {
        normal = -normal;
    }
    let m = materials[bitcast<u32>(shade.geometric.w)];
    let kind = bitcast<u32>(m.kind.x);
    var color = m.color.xyz;
    var alpha = -1.0;
    if kind == 1u {
        alpha = max(m.kind.y * m.kind.y, 1e-4);
    } else if kind == 2u {
        let at = vec3<f32>(
            dot(m.row0.xyz, position) + m.row0.w,
            dot(m.row1.xyz, position) + m.row1.w,
            dot(m.row2.xyz, position) + m.row2.w,
        );
        let end = smoothed(0.55, 0.85, abs(dot(normal, m.along.xyz)));
        color = wood(m, at) * FINISH_TINT * (1.0 + (END_GRAIN - 1.0) * end);
        let roughness = SATIN + (SATIN_ON_END_GRAIN - SATIN) * end;
        alpha = max(roughness * roughness, 1e-4);
    }
    // A frame around the normal, without a seam (Duff et al. 2017).
    let side = select(-1.0, 1.0, normal.z >= 0.0);
    let a = -1.0 / (side + normal.z);
    let b = normal.x * normal.y * a;
    let tangent = vec3<f32>(1.0 + side * normal.x * normal.x * a, side * b, -side * normal.x);
    let bitangent = vec3<f32>(b, side + normal.y * normal.y * a, -normal.y);
    return Surface(position, geometric, tangent, bitangent, normal, color, alpha);
}

fn power_heuristic(a: f32, b: f32) -> f32 {
    let a2 = a * a;
    let b2 = b * b;
    if a2 + b2 > 0.0 {
        return a2 / (a2 + b2);
    }
    return 0.0;
}

fn clamped(light: vec3<f32>, bounce: u32) -> vec3<f32> {
    let most = max_element(light);
    if bounce == 0u || most <= BRIGHTEST_BOUNCE {
        return light;
    }
    return light * (BRIGHTEST_BOUNCE / most);
}

// The light that comes back along one path from the camera.
fn path(camera: Ray, seed: u32, index: u32) -> vec3<f32> {
    var rng = scramble(seed ^ scramble(index + 0x5bd1e995u));
    var ray = camera;
    var gathered = vec3<f32>(0.0);
    var throughput = vec3<f32>(1.0);
    var last_pdf = -1.0;
    let count = params.counts.x;
    for (var bounce = 0u; bounce < MAX_BOUNCES; bounce++) {
        let hit = walk(ray, 3.0e38, false);
        var t_surface = 3.0e38;
        if hit.found {
            t_surface = hit.t;
        }
        if last_pdf >= 0.0 {
            var nearest = t_surface;
            var found = -1;
            var cosine = 0.0;
            for (var k = 0u; k < count; k++) {
                let seen = hit_lamp(lamps[k], ray, nearest);
                if seen.x > 0.0 {
                    nearest = seen.x;
                    found = i32(k);
                    cosine = seen.y;
                }
            }
            if found >= 0 {
                let lamp = lamps[found];
                let lamp_pdf = nearest * nearest / (cosine * lamp_area(lamp)) / f32(count);
                gathered += clamped(throughput * lamp.radiance.xyz * power_heuristic(last_pdf, lamp_pdf), bounce);
                break;
            }
        }
        if !hit.found {
            gathered += clamped(throughput * environment(ray.dir), bounce);
            break;
        }
        let s = surface_at(hit, ray);
        let wo = -ray.dir;
        let u = sobol4(index, scramble(seed ^ ((bounce + 1u) * 0x68e31da4u)));
        // Light from a lamp, straight to here.
        if count > 0u {
            let lamp = lamps[u32(next_random(&rng) * f32(count)) % count];
            let point = lamp.center.xyz + lamp.u.xyz * (2.0 * u.z - 1.0) + lamp.v.xyz * (2.0 * u.w - 1.0);
            let to = point - s.position;
            let reach = length(to);
            let wi = to / reach;
            let cosine = -dot(wi, lamp_normal(lamp));
            if cosine > 0.0 && dot(wi, s.geometric) > 0.0 {
                let f = eval(s, wo, wi);
                if max_element(f.xyz) > 0.0 && !walk(leaving(s, wi), reach * (1.0 - 1e-4), true).found {
                    let lamp_pdf = reach * reach / (cosine * lamp_area(lamp)) / f32(count);
                    gathered += clamped(
                        throughput * f.xyz * lamp.radiance.xyz * power_heuristic(lamp_pdf, f.w) / lamp_pdf,
                        bounce,
                    );
                }
            }
        }
        // On to wherever the light bounces.
        let next = sample_bounce(s, wo, u.xy, next_random(&rng));
        if !next.ok {
            break;
        }
        throughput *= next.weight;
        last_pdf = next.pdf;
        ray = leaving(s, next.wi);
        if bounce >= 3u {
            let survive = min(max_element(throughput), 0.95);
            if next_random(&rng) >= survive {
                break;
            }
            throughput /= survive;
        }
    }
    return gathered;
}

@compute @workgroup_size(8, 8)
fn trace(@builtin(global_invocation_id) id: vec3<u32>) {
    let width = params.size.x;
    let height = params.size.y;
    let x = id.x;
    let y = id.y + params.counts.z;
    if x >= width || y >= height {
        return;
    }
    let seed = scramble((x * 0x8da6b343u) ^ (y * 0xd8163841u) ^ 0x2c1b3c6du);
    var sum = vec3<f32>(0.0);
    for (var index = params.size.z; index < params.size.w; index++) {
        let s = sobol4(index, seed);
        let at = vec2<f32>(
            (f32(x) + 0.5 + tent(s.x)) / f32(width) * 2.0 - 1.0,
            1.0 - (f32(y) + 0.5 + tent(s.y)) / f32(height) * 2.0,
        );
        let sample = path(camera_ray(at, s.zw), seed, index);
        if finite(sample) {
            sum += sample;
        }
    }
    let i = y * width + x;
    light[i] += vec4<f32>(sum, 0.0);
}

// ---- Developing ----

fn tone_map(light_in: vec3<f32>) -> vec3<f32> {
    let start = 0.8 - 0.04;
    let desaturation = 0.15;
    let least = min(min(light_in.x, light_in.y), light_in.z);
    var offset = 0.04;
    if least < 0.08 {
        offset = least - 6.25 * least * least;
    }
    var color = light_in - vec3<f32>(offset);
    let peak = max_element(color);
    if peak >= start {
        let d = 1.0 - start;
        let new_peak = 1.0 - d * d / (peak + d - start);
        color = color * (new_peak / peak);
        let g = 1.0 - 1.0 / (desaturation * (peak - new_peak) + 1.0);
        color = mix(color, vec3<f32>(new_peak), g);
    }
    let c = max(color, vec3<f32>(0.0));
    let low = 12.92 * c;
    let high = 1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - vec3<f32>(0.055);
    return select(high, low, c <= vec3<f32>(0.0031308));
}

@compute @workgroup_size(8, 8)
fn develop(@builtin(global_invocation_id) id: vec3<u32>) {
    let width = params.size.x;
    let height = params.size.y;
    if id.x >= width || id.y >= height {
        return;
    }
    let i = id.y * width + id.x;
    let middle = vec2<f32>(f32(width), f32(height)) / 2.0;
    let off = (vec2<f32>(f32(id.x), f32(id.y)) - middle) / length(middle);
    let vignette = 1.0 - 0.35 * pow(dot(off, off), 1.5);
    let scale = params.develop.x / max(params.develop.y, 1.0);
    let color = tone_map(light[i].xyz * scale * vignette);
    let dither = f32(scramble(i) >> 24u) / 255.0 - 0.5;
    let bytes = vec3<u32>(clamp(round(color * 255.0 + vec3<f32>(dither)), vec3<f32>(0.0), vec3<f32>(255.0)));
    pixels[i] = bytes.x | (bytes.y << 8u) | (bytes.z << 16u) | (255u << 24u);
}
