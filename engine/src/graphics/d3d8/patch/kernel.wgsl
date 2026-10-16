@group(0) @binding(0) var<storage, read> params: array<u32>;
@group(0) @binding(1) var<storage, read> raw: array<u32>;
@group(0) @binding(2) var<storage, read_write> controls: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read> domain: array<vec2<f32>>;
@group(0) @binding(4) var<storage, read_write> vertices: array<vec4<f32>>;

fn byte_at(address: u32) -> u32 {
    return (raw[address / 4u] >> ((address % 4u) * 8u)) & 255u;
}
fn word_at(address: u32) -> u32 {
    return byte_at(address) | (byte_at(address + 1u) << 8u)
        | (byte_at(address + 2u) << 16u) | (byte_at(address + 3u) << 24u);
}
fn short_at(address: u32) -> f32 {
    let word = byte_at(address) | (byte_at(address + 1u) << 8u);
    return f32(bitcast<i32>(word << 16u) >> 16u);
}
fn decode(address: u32, kind: u32) -> vec4<f32> {
    var value = vec4<f32>(0.0, 0.0, 0.0, 1.0);
    if kind <= 3u {
        for (var lane = 0u; lane <= kind; lane++) {
            value[lane] = bitcast<f32>(word_at(address + 4u * lane));
        }
    } else if kind == 4u {
        let color = word_at(address);
        value = vec4<f32>(f32((color >> 16u) & 255u), f32((color >> 8u) & 255u),
            f32(color & 255u), f32(color >> 24u)) / 255.0;
    } else if kind == 5u {
        value = vec4<f32>(f32(byte_at(address)), f32(byte_at(address + 1u)),
            f32(byte_at(address + 2u)), f32(byte_at(address + 3u)));
    } else {
        let count = select(2u, 4u, kind == 7u);
        for (var lane = 0u; lane < count; lane++) {
            value[lane] = short_at(address + 2u * lane);
        }
    }
    return value;
}
@compute @workgroup_size(64)
fn capture(@builtin(global_invocation_id) id: vec3<u32>) {
    let point = id.x;
    if point >= params[6] * params[7] { return; }
    let source = params[1] + (point / params[6]) * params[2] + point % params[6];
    for (var reg = 0u; reg < 17u; reg++) {
        controls[point * 17u + reg] = vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }
    for (var element = 0u; element < params[128]; element++) {
        let at = 132u + 4u * element;
        let stream = 256u + 2u * params[at + 1u];
        let address = params[stream] + source * params[stream + 1u] + params[at];
        controls[point * 17u + params[at + 3u]] = decode(address, params[at + 2u]);
    }
}
fn polynomial(index: u32, t: f32) -> vec2<f32> {
    let at = 8u + index * 6u;
    var value = bitcast<f32>(params[at + params[3]]);
    var slope = 0.0;
    for (var power = i32(params[3]) - 1; power >= 0; power--) {
        slope = slope * t + value;
        value = value * t + bitcast<f32>(params[at + u32(power)]);
    }
    return vec2<f32>(value, slope);
}
fn integer_power(value: f32, power: u32) -> f32 {
    var result = 1.0;
    for (var i = 0u; i < power; i++) { result *= value; }
    return result;
}
fn derivative(value: f32, power: u32) -> f32 {
    if power == 0u { return 0.0; }
    return f32(power) * integer_power(value, power - 1u);
}
@compute @workgroup_size(64)
fn evaluate(@builtin(global_invocation_id) id: vec3<u32>) {
    let vertex = id.x;
    if vertex >= arrayLength(&domain) { return; }
    let uv = domain[vertex];
    var values: array<vec4<f32>, 17>;
    var du: array<vec4<f32>, 17>;
    var dv: array<vec4<f32>, 17>;
    for (var reg = 0u; reg < 17u; reg++) {
        values[reg] = vec4<f32>(0.0); du[reg] = vec4<f32>(0.0); dv[reg] = vec4<f32>(0.0);
    }
    if params[0] == 0u {
        let scaled = uv * vec2<f32>(f32(params[4]), f32(params[5]));
        let cell = min(vec2<u32>(scaled), vec2<u32>(params[4] - 1u, params[5] - 1u));
        let local = scaled - vec2<f32>(cell);
        for (var row = 0u; row <= params[3]; row++) {
            let b = polynomial(row, local.y);
            for (var col = 0u; col <= params[3]; col++) {
                let a = polynomial(col, local.x);
                let point = (cell.y + row) * params[6] + cell.x + col;
                for (var reg = 0u; reg < 17u; reg++) {
                    let control = controls[point * 17u + reg];
                    values[reg] += a.x * b.x * control;
                    du[reg] += a.y * b.x * f32(params[4]) * control;
                    dv[reg] += a.x * b.y * f32(params[5]) * control;
                }
            }
        }
    } else {
        let w = 1.0 - uv.x - uv.y;
        for (var point = 0u; point < params[6]; point++) {
            let at = 8u + point * 4u;
            let a = params[at]; let b = params[at + 1u]; let c = params[at + 2u];
            let k = bitcast<f32>(params[at + 3u]);
            let u0 = integer_power(uv.x, a); let v0 = integer_power(uv.y, b);
            let w0 = integer_power(w, c); let dw = derivative(w, c);
            let weight = k * u0 * v0 * w0;
            let weight_u = k * v0 * (derivative(uv.x, a) * w0 - u0 * dw);
            let weight_v = k * u0 * (derivative(uv.y, b) * w0 - v0 * dw);
            for (var reg = 0u; reg < 17u; reg++) {
                let control = controls[point * 17u + reg];
                values[reg] += weight * control;
                du[reg] += weight_u * control; dv[reg] += weight_v * control;
            }
        }
    }
    for (var generated = 0u; generated < params[129]; generated++) {
        let at = 200u + generated * 3u;
        let destination = params[at + 2u];
        if params[at] == 0u {
            let source_register = params[at + 1u];
            let normal = cross(du[source_register].xyz, dv[source_register].xyz);
            let length2 = dot(normal, normal);
            var unit = vec3<f32>(0.0);
            if length2 > 0.0 { unit = normal * inverseSqrt(length2); }
            values[destination] = vec4<f32>(unit, 1.0);
        } else {
            values[destination] = vec4<f32>(uv, 0.0, 1.0);
        }
    }
    for (var reg = 0u; reg < 17u; reg++) {
        vertices[vertex * 17u + reg] = values[reg];
    }
}
