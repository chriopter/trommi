// The view model's types for the shells, written from the Rust types, so no shell writes them by hand.
//
// The view model is declared once, in Rust (core/client/src/views/mod.rs: every `pub struct` and `pub enum` with their
// serde attributes). This script reads those declarations and writes
//   - TypeScript: core/bindings/wasm/js/views.d.ts (interfaces and unions of the JSON the core sends and takes),
//   - Swift: core/bindings/swift/TrommiCoreRust/Sources/TrommiCoreRust/Views.swift (Codable structs and enums),
// and checks that the files in the repository are what it writes. It also reads every golden view of the demo room
// (core/client/tests/golden/*.json) against the declarations, strictly: every key known, every field the core always
// writes present, every value of its type. So a serde attribute this script misreads fails here, not in a shell.
//
//   node tests/bindings/views.mjs            check
//   node tests/bindings/views.mjs --write    write both files, then check
//
// What it understands is what views/mod.rs uses: structs with named fields (generic ones too), enums of unit variants
// (strings), internally tagged enums (`tag = "…"`) of unit and struct variants, `rename_all = "snake_case"`, the field
// attributes `default`, `default = "yes"` and `skip_serializing_if`, and the types String, bool, the integers, f64,
// Option, Vec, Box, BTreeMap<String, _> and the module's own types. A type of another module is carried as plain JSON
// (`crate::actions::Action`: a toast's action is sent back as it came). Anything else stops the script with its line.
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const repo = path.join(path.dirname(fileURLToPath(import.meta.url)), '../..')
const SOURCE = 'core/client/src/views/mod.rs'
const TS_OUT = 'core/bindings/wasm/js/views.d.ts'
const SWIFT_OUT = 'core/bindings/swift/TrommiCoreRust/Sources/TrommiCoreRust/Views.swift'
const GOLDEN = 'core/client/tests/golden'
const read = file => fs.readFileSync(path.join(repo, file), 'utf8')
const problems = []
const problem = text => problems.push(text)
const fail = (line, text) => { throw new Error(`${SOURCE}:${line}: ${text}`) }

// ---- reading the Rust declarations ----------------------------------------------------------------------------------

/** The source without the tests, line by line, `//` comments out but `///` docs kept. */
function lines() {
  const all = read(SOURCE).split('\n')
  const end = all.findIndex((l, i) => l.startsWith('#[cfg(test)]') && all[i + 1]?.startsWith('mod tests'))
  return (end < 0 ? all : all.slice(0, end)).map((text, i) => ({ n: i + 1, text }))
}

/** Splits at the commas that are not inside <>, (), [] or {}. */
function split(text) {
  const parts = []
  let depth = 0, start = 0
  for (let at = 0; at < text.length; at++) {
    if ('<([{'.includes(text[at])) depth++
    if ('>)]}'.includes(text[at])) depth--
    if (text[at] === ',' && depth === 0) { parts.push(text.slice(start, at)); start = at + 1 }
  }
  parts.push(text.slice(start))
  return parts
}

/** A Rust type as a tree: { k: 'string' | 'bool' | 'int' | 'float' | 'json' | 'option' | 'list' | 'map' | 'named', … }. */
function typeOf(text, line, params) {
  const t = text.trim().replace(/\s+/g, '')
  const inner = (prefix) => t.slice(prefix.length, -1)
  if (t === 'String') return { k: 'string' }
  if (t === 'bool') return { k: 'bool' }
  if (/^[ui](8|16|32|64)$/.test(t)) return { k: 'int', rust: t }
  if (t === 'f64' || t === 'f32') return { k: 'float' }
  if (t.startsWith('crate::')) return { k: 'json', rust: t }
  if (t.startsWith('Option<')) return { k: 'option', of: typeOf(inner('Option<'), line, params) }
  if (t.startsWith('Vec<')) return { k: 'list', of: typeOf(inner('Vec<'), line, params) }
  if (t.startsWith('Box<')) return typeOf(inner('Box<'), line, params)
  if (t.startsWith('BTreeMap<String,')) return { k: 'map', of: typeOf(t.slice('BTreeMap<String,'.length, -1), line, params) }
  if (params.includes(t)) return { k: 'param', name: t }
  const m = /^([A-Z][A-Za-z0-9]*)(?:<(.+)>)?$/.exec(t)
  if (m) return { k: 'named', name: m[1], args: m[2] ? split(m[2]).map(a => typeOf(a, line, params)) : [] }
  fail(line, `a type this script does not know: ${text.trim()}`)
}

/** The serde attribute's settings: `default`, `default = "yes"`, `skip_serializing_if = "…"`, `tag`, `rename_all`. */
function serdeOf(attrs, line) {
  const out = {}
  for (const a of attrs) {
    const m = /^#\[serde\((.*)\)\]$/.exec(a)
    if (!m) continue
    for (const part of split(m[1]).map(p => p.trim()).filter(Boolean)) {
      const kv = /^(\w+)(?:\s*=\s*"([^"]*)")?$/.exec(part)
      if (!kv || !['default', 'skip_serializing_if', 'tag', 'rename_all'].includes(kv[1])) fail(line, `a serde attribute this script does not know: ${part}`)
      out[kv[1]] = kv[2] ?? true
    }
  }
  if (out.rename_all && out.rename_all !== 'snake_case') fail(line, `rename_all = "${out.rename_all}"`)
  if (typeof out.default === 'string' && out.default !== 'yes') fail(line, `default = "${out.default}": add it to this script`)
  return out
}

const snake = name => name.replace(/([a-z0-9])([A-Z])/g, '$1_$2').toLowerCase()

/** The pieces of a body (fields or variants), each with its docs and attributes. */
function pieces(body, line) {
  // (commas inside comments do not split: they are found on the text with its comments blanked out)
  const masked = body.replace(/\/\/[^\n]*/g, m => ' '.repeat(m.length))
  const raws = []
  let start = 0
  for (const part of split(masked)) { raws.push(body.slice(start, start + part.length)); start += part.length + 1 }
  return raws.map(raw => {
    const docs = [], attrs = []
    let rest = []
    // (what follows the first line of code is its own, a variant's fields with their docs and attributes: kept as is)
    for (const l of raw.split('\n')) {
      const s = l.trim()
      if (rest.length) rest.push(l)
      else if (s.startsWith('///')) docs.push(s.replace(/^\/\/\/ ?/, ''))
      else if (s.startsWith('//')) continue
      else if (s.startsWith('#[')) attrs.push(s)
      else if (s) rest.push(s)
    }
    return { docs, attrs, text: rest.join('\n').trim(), line }
  }).filter(p => p.text)
}

function fieldsOf(body, line, params) {
  return pieces(body, line).map(p => {
    const m = /^(?:pub\s+)?(\w+)\s*:\s*(.+)$/s.exec(p.text)
    if (!m) fail(line, `a field this script does not read: ${p.text}`)
    return { name: m[1], type: typeOf(m[2], line, params), serde: serdeOf(p.attrs, line), docs: p.docs }
  })
}

/** Every `pub struct` and `pub enum` of the module, in order. */
function declarations() {
  const ls = lines()
  const out = []
  let docs = [], attrs = []
  for (let i = 0; i < ls.length; i++) {
    const { n, text } = ls[i]
    const s = text.trim()
    if (text.startsWith('///')) { docs.push(s.replace(/^\/\/\/ ?/, '')); continue }
    if (text.startsWith('#[')) { attrs.push(s); continue }
    const head = /^pub (struct|enum) (\w+)(?:<([^>]+)>)?\s*(\{|;)?/.exec(text)
    if (!head) {
      // (an impl, a fn, a use: its body is skipped)
      if (/^(impl|fn|pub fn|mod|pub mod|pub use|use|const|pub const)\b/.test(text) && text.trimEnd().endsWith('{')) {
        let depth = 0
        for (; i < ls.length; i++) {
          for (const c of ls[i].text.replace(/\/\/.*$/, '')) { if (c === '{') depth++; if (c === '}') depth-- }
          if (depth === 0) break
        }
      }
      if (!text.startsWith('//')) { docs = []; attrs = [] }
      continue
    }
    const [, kind, name, generics, open] = head
    const params = generics ? generics.split(',').map(g => g.trim().split(':')[0].trim()) : []
    if (open !== '{') fail(n, `${name}: only a body in braces is read`)
    // (braces in comments do not count)
    let depth = 0, body = ''
    for (; i < ls.length; i++) {
      const t = ls[i].text, code = t.indexOf('//') < 0 ? t.length : t.indexOf('//')
      for (let at = 0; at < t.length; at++) {
        const c = t[at]
        if (at < code && c === '{') { depth++; if (depth === 1) continue }
        if (at < code && c === '}') { depth--; if (depth === 0) break }
        if (depth >= 1) body += c
      }
      if (depth >= 1) body += '\n'
      if (depth === 0) break
    }
    const serde = serdeOf(attrs, n)
    const derives = attrs.filter(a => a.startsWith('#[derive')).join(' ')
    if (!/Serialize/.test(derives)) { docs = []; attrs = []; continue }
    if (kind === 'struct') {
      out.push({ kind, name, params, docs, serde, line: n, fields: fieldsOf(body, n, params), defaulted: /Default/.test(derives) })
    } else {
      const variants = pieces(body, n).map(p => {
        const m = /^(\w+)\s*(?:\{(.*)\})?$/s.exec(p.text)
        if (!m) fail(n, `${name}: a variant this script does not read: ${p.text}`)
        return { name: m[1], wire: serde.rename_all ? snake(m[1]) : m[1], docs: p.docs, isDefault: p.attrs.includes('#[default]'), fields: m[2] != null ? fieldsOf(m[2], n, params) : null }
      })
      if (!serde.tag && variants.some(v => v.fields)) fail(n, `${name}: an enum with fields needs a tag`)
      out.push({ kind, name, params, docs, serde, line: n, variants })
    }
    docs = []; attrs = []
  }
  return out
}

// ---- what a field may be on the wire ----------------------------------------------------------------------------------
// required: always written, and needed when read; else it may be left out (skip_serializing_if) and is then its
// default when read. An Option without skip_serializing_if is written as null.
const mayBeAbsent = f => Boolean(f.serde.skip_serializing_if) || Boolean(f.serde.default)

// ---- TypeScript ---------------------------------------------------------------------------------------------------------

function ts(t) {
  switch (t.k) {
    case 'string': return 'string'
    case 'bool': return 'boolean'
    case 'int': case 'float': return 'number'
    case 'json': return 'unknown'
    case 'option': return `${ts(t.of)} | null`
    case 'list': return `${wrap(ts(t.of))}[]`
    case 'map': return `Record<string, ${ts(t.of)}>`
    case 'param': return t.name
    case 'named': return t.args.length ? `${t.name}<${t.args.map(ts).join(', ')}>` : t.name
  }
}
const wrap = s => (s.includes(' ') ? `(${s})` : s)
const tsDocs = (docs, pad) => (docs.length ? `${pad}/** ${docs.join(`\n${pad} *  `)} */\n` : '')

function tsField(f, pad) {
  // (left out when empty or none: optional; an Option written as null when it is none)
  const optional = Boolean(f.serde.skip_serializing_if)
  const type = optional && f.type.k === 'option' ? ts(f.type.of) : ts(f.type)
  return `${tsDocs(f.docs, pad)}${pad}${f.name}${optional ? '?' : ''}: ${type}\n`
}

function typescript(decls) {
  let out = `// Written by tests/bindings/views.mjs from ${SOURCE}: do not edit. The view model the core sends (Effect::Render)\n// and the parts of it a shell sends back (Event::Focus). A field marked ? is left out when it is empty or none.\n\n`
  for (const d of decls) {
    const gen = d.params.length ? `<${d.params.join(', ')}>` : ''
    out += tsDocs(d.docs, '')
    if (d.kind === 'struct') {
      out += `export interface ${d.name}${gen} {\n${d.fields.map(f => tsField(f, '  ')).join('')}}\n\n`
    } else if (!d.serde.tag) {
      out += `export type ${d.name} = ${d.variants.map(v => `'${v.wire}'`).join(' | ')}\n\n`
    } else {
      const arms = d.variants.map(v => `${tsDocs(v.docs, '  ')}  | { ${d.serde.tag}: '${v.wire}'${(v.fields ?? []).map(f => `; ${f.name}${f.serde.skip_serializing_if ? '?' : ''}: ${f.serde.skip_serializing_if && f.type.k === 'option' ? ts(f.type.of) : ts(f.type)}`).join('')} }\n`)
      out += `export type ${d.name}${gen} =\n${arms.join('')}\n`
    }
  }
  return out.trimEnd() + '\n'
}

// ---- Swift --------------------------------------------------------------------------------------------------------------

const SWIFT_WORDS = new Set(['as', 'break', 'case', 'catch', 'class', 'continue', 'default', 'defer', 'do', 'else', 'enum', 'extension', 'fallthrough', 'false', 'for', 'func', 'guard', 'if', 'import', 'in', 'init', 'inout', 'internal', 'is', 'let', 'nil', 'operator', 'private', 'protocol', 'public', 'repeat', 'return', 'self', 'Self', 'static', 'struct', 'subscript', 'super', 'switch', 'throw', 'throws', 'true', 'try', 'typealias', 'var', 'where', 'while', 'Type', 'Protocol'])
const camel = name => name.replace(/_([a-z0-9])/g, (_, c) => c.toUpperCase())
const swiftName = name => (SWIFT_WORDS.has(camel(name)) ? `\`${camel(name)}\`` : camel(name))
const bare = name => camel(name)
const INTS = { u8: 'UInt8', u16: 'UInt16', u32: 'UInt32', u64: 'UInt64', i8: 'Int8', i16: 'Int16', i32: 'Int32', i64: 'Int64' }

function swift(t) {
  switch (t.k) {
    case 'string': return 'String'
    case 'bool': return 'Bool'
    case 'int': return INTS[t.rust]
    case 'float': return 'Double'
    case 'json': return 'CoreJSON'
    case 'option': return `${swift(t.of)}?`
    case 'list': return `[${swift(t.of)}]`
    case 'map': return `[String: ${swift(t.of)}]`
    case 'param': return t.name
    case 'named': return t.args.length ? `${t.name}<${t.args.map(swift).join(', ')}>` : t.name
  }
}

/** The value a field takes when it is left out, in Swift; null when it has none (then it is required). */
function swiftDefault(f, decls) {
  if (f.serde.default === 'yes') return 'true'
  const t = f.type
  switch (t.k) {
    case 'option': return 'nil'
    case 'string': return '""'
    case 'bool': return 'false'
    case 'int': case 'float': return '0'
    case 'list': return '[]'
    case 'map': return '[:]'
    case 'named': {
      const d = decls.find(x => x.name === t.name)
      if (d?.kind === 'struct' && d.defaulted) return `${t.name}()`
      if (d?.kind === 'enum' && d.variants.some(v => v.isDefault)) {
        const v = d.variants.find(x => x.isDefault)
        return v.fields ? null : `.${swiftCase(v.name)}`
      }
      return null
    }
    default: return null
  }
}
const swiftCase = name => { const c = name[0].toLowerCase() + name.slice(1); return SWIFT_WORDS.has(c) ? `\`${c}\`` : c }
const swiftDocs = (docs, pad) => docs.map(d => `${pad}/// ${d}\n`).join('')

function swiftStruct(d, decls) {
  const gen = d.params.length ? `<${d.params.map(p => `${p}: Codable & Hashable & Sendable`).join(', ')}>` : ''
  const ty = `${d.name}${d.params.length ? `<${d.params.join(', ')}>` : ''}`
  let out = `${swiftDocs(d.docs, '')}public struct ${d.name}${gen}: Codable, Hashable, Sendable {\n`
  for (const f of d.fields) out += `${swiftDocs(f.docs, '  ')}  public var ${swiftName(f.name)}: ${swift(f.type)}\n`
  // (a memberwise init: what has a default may be left out)
  const args = d.fields.map(f => { const def = d.defaulted ? swiftDefault(f, decls) : null; return `${swiftName(f.name)}: ${swift(f.type)}${def != null ? ` = ${def}` : ''}` })
  out += `\n  public init(${args.join(', ')}) {\n${d.fields.map(f => `    self.${bare(f.name)} = ${swiftName(f.name)}\n`).join('')}  }\n`
  out += `\n  enum CodingKeys: String, CodingKey {\n${d.fields.map(f => `    case ${swiftName(f.name)} = "${f.name}"\n`).join('')}  }\n`
  out += `\n  public init(from decoder: any Decoder) throws {\n    let c = try decoder.container(keyedBy: CodingKeys.self)\n`
  for (const f of d.fields) {
    const def = mayBeAbsent(f) ? swiftDefault(f, decls) : null
    if (mayBeAbsent(f) && def == null) fail(d.line, `${d.name}.${f.name} may be left out but has no default in Swift`)
    if (f.type.k === 'option') out += `    self.${bare(f.name)} = try c.decodeIfPresent(${swift(f.type.of)}.self, forKey: .${bare(f.name)})\n`
    else if (def != null) out += `    self.${bare(f.name)} = try c.decodeIfPresent(${swift(f.type)}.self, forKey: .${bare(f.name)}) ?? ${def}\n`
    else out += `    self.${bare(f.name)} = try c.decode(${swift(f.type)}.self, forKey: .${bare(f.name)})\n`
  }
  out += `  }\n\n  public func encode(to encoder: any Encoder) throws {\n    var c = encoder.container(keyedBy: CodingKeys.self)\n`
  for (const f of d.fields) {
    const skip = f.serde.skip_serializing_if
    const n = bare(f.name)
    if (f.type.k === 'option') out += skip ? `    try c.encodeIfPresent(${n}, forKey: .${n})\n` : `    try c.encode(${n}, forKey: .${n})\n`
    else if (skip && /is_empty/.test(skip)) out += `    if !${n}.isEmpty { try c.encode(${n}, forKey: .${n}) }\n`
    else if (skip) fail(d.line, `${d.name}.${f.name}: skip_serializing_if = "${skip}"`)
    else out += `    try c.encode(${n}, forKey: .${n})\n`
  }
  out += `  }\n}\n\n`
  void ty
  return out
}

function swiftEnum(d) {
  if (!d.serde.tag) {
    let out = `${swiftDocs(d.docs, '')}public enum ${d.name}: String, Codable, Hashable, Sendable {\n`
    for (const v of d.variants) out += `${swiftDocs(v.docs, '  ')}  case ${swiftCase(v.name)} = "${v.wire}"\n`
    const def = d.variants.find(v => v.isDefault)
    if (def) out += `\n  public init() { self = .${swiftCase(def.name)} }\n`
    return out + `}\n\n`
  }
  const gen = d.params.length ? `<${d.params.map(p => `${p}: Codable & Hashable & Sendable`).join(', ')}>` : ''
  let out = `${swiftDocs(d.docs, '')}public enum ${d.name}${gen}: Codable, Hashable, Sendable {\n`
  for (const v of d.variants) {
    const assoc = v.fields ? `(${v.fields.map(f => `${bare(f.name)}: ${swift(f.type)}`).join(', ')})` : ''
    out += `${swiftDocs(v.docs, '  ')}  case ${swiftCase(v.name)}${assoc}\n`
  }
  const def = d.variants.find(v => v.isDefault && !v.fields)
  if (def) out += `\n  public init() { self = .${swiftCase(def.name)} }\n`
  const keys = [...new Set([d.serde.tag, ...d.variants.flatMap(v => (v.fields ?? []).map(f => f.name))])]
  out += `\n  enum CodingKeys: String, CodingKey {\n${keys.map(k => `    case ${swiftName(k)} = "${k}"\n`).join('')}  }\n`
  out += `\n  public init(from decoder: any Decoder) throws {\n    let c = try decoder.container(keyedBy: CodingKeys.self)\n    switch try c.decode(String.self, forKey: .${bare(d.serde.tag)}) {\n`
  for (const v of d.variants) {
    const args = (v.fields ?? []).map(f => {
      const def = mayBeAbsent(f) ? (f.serde.default === 'yes' ? 'true' : swiftDefault(f, [])) : null
      if (f.type.k === 'option') return `${bare(f.name)}: try c.decodeIfPresent(${swift(f.type.of)}.self, forKey: .${bare(f.name)})`
      if (def != null) return `${bare(f.name)}: try c.decodeIfPresent(${swift(f.type)}.self, forKey: .${bare(f.name)}) ?? ${def}`
      return `${bare(f.name)}: try c.decode(${swift(f.type)}.self, forKey: .${bare(f.name)})`
    })
    out += `    case "${v.wire}": self = .${swiftCase(v.name)}${v.fields ? `(${args.join(', ')})` : ''}\n`
  }
  out += `    case let other: throw DecodingError.dataCorruptedError(forKey: .${bare(d.serde.tag)}, in: c, debugDescription: "${d.name}: no variant \\(other)")\n    }\n  }\n`
  out += `\n  public func encode(to encoder: any Encoder) throws {\n    var c = encoder.container(keyedBy: CodingKeys.self)\n    switch self {\n`
  for (const v of d.variants) {
    const binds = v.fields ? `(${v.fields.map(f => `let ${bare(f.name)}`).join(', ')})` : ''
    out += `    case .${swiftCase(v.name)}${binds}:\n      try c.encode("${v.wire}", forKey: .${bare(d.serde.tag)})\n`
    for (const f of v.fields ?? []) {
      const n = bare(f.name)
      if (f.type.k === 'option' && f.serde.skip_serializing_if) out += `      try c.encodeIfPresent(${n}, forKey: .${n})\n`
      else out += `      try c.encode(${n}, forKey: .${n})\n`
    }
  }
  return out + `    }\n  }\n}\n\n`
}

const CORE_JSON = `/// A value the views carry as plain JSON, to be sent back as it came (a toast's action).
public enum CoreJSON: Codable, Hashable, Sendable {
  case null
  case bool(Bool)
  case number(Double)
  case string(String)
  case array([CoreJSON])
  case object([String: CoreJSON])

  public init(from decoder: any Decoder) throws {
    let c = try decoder.singleValueContainer()
    if c.decodeNil() { self = .null }
    else if let b = try? c.decode(Bool.self) { self = .bool(b) }
    else if let n = try? c.decode(Double.self) { self = .number(n) }
    else if let s = try? c.decode(String.self) { self = .string(s) }
    else if let a = try? c.decode([CoreJSON].self) { self = .array(a) }
    else { self = .object(try c.decode([String: CoreJSON].self)) }
  }

  public func encode(to encoder: any Encoder) throws {
    var c = encoder.singleValueContainer()
    switch self {
    case .null: try c.encodeNil()
    case .bool(let b): try c.encode(b)
    case .number(let n): try c.encode(n)
    case .string(let s): try c.encode(s)
    case .array(let a): try c.encode(a)
    case .object(let o): try c.encode(o)
    }
  }
}
`

function swiftFile(decls) {
  let body = ''
  for (const d of decls) body += d.kind === 'struct' ? swiftStruct(d, decls) : swiftEnum(d)
  body += CORE_JSON
  // (inside one namespace: a view type never meets SwiftUI's Text or an app's own Toast)
  const inner = body.trimEnd().split('\n').map(l => (l ? `  ${l}` : l)).join('\n')
  return `// Written by tests/bindings/views.mjs from ${SOURCE}: do not edit. The view model the core sends (Effect::Render)\n// and the parts of it a shell sends back (Event::Focus), as Codable types: a field the core may leave out decodes to\n// its default. Every type is inside \`CoreView\` (\`CoreView.DeskView\`), so none meets a name of SwiftUI or of an app.\nimport Foundation\n\npublic enum CoreView {\n${inner}\n}\n`
}

// ---- the golden views, read strictly --------------------------------------------------------------------------------

function check(value, t, at, decls, bind = {}) {
  const is = (ok, what) => { if (!ok) problem(`${at}: not ${what}: ${JSON.stringify(value)?.slice(0, 80)}`) }
  switch (t.k) {
    case 'string': return is(typeof value === 'string', 'a string')
    case 'bool': return is(typeof value === 'boolean', 'a bool')
    case 'int': return is(Number.isInteger(value) && (t.rust.startsWith('i') || value >= 0), `an integer ${t.rust}`)
    case 'float': return is(typeof value === 'number', 'a number')
    case 'json': return
    case 'option': return value === null ? undefined : check(value, t.of, at, decls, bind)
    case 'list': is(Array.isArray(value), 'a list'); return Array.isArray(value) && value.forEach((v, i) => check(v, t.of, `${at}[${i}]`, decls, bind))
    case 'map': is(value && typeof value === 'object' && !Array.isArray(value), 'a map'); return value && Object.entries(value).forEach(([k, v]) => check(v, t.of, `${at}.${k}`, decls, bind))
    case 'param': return check(value, bind[t.name], at, decls, bind)
    case 'named': {
      const d = decls.find(x => x.name === t.name)
      if (!d) return problem(`${at}: ${t.name} is not declared`)
      const inner = Object.fromEntries(d.params.map((p, i) => [p, t.args[i]]))
      const fields = (fields, obj, skip = []) => {
        for (const k of Object.keys(obj)) if (!skip.includes(k) && !fields.some(f => f.name === k)) problem(`${at}.${k}: no such field of ${d.name}`)
        for (const f of fields) {
          if (!(f.name in obj)) { if (!f.serde.skip_serializing_if) problem(`${at}.${f.name}: missing (${d.name} always writes it)`); continue }
          check(obj[f.name], f.type, `${at}.${f.name}`, decls, inner)
        }
      }
      if (d.kind === 'struct') { is(value && typeof value === 'object', `a ${d.name}`); return value && fields(d.fields, value) }
      if (!d.serde.tag) return is(d.variants.some(v => v.wire === value), `one of ${d.name}`)
      const v = d.variants.find(x => x.wire === value?.[d.serde.tag])
      if (!v) return problem(`${at}: no variant of ${d.name}: ${value?.[d.serde.tag]}`)
      return fields(v.fields ?? [], value, [d.serde.tag])
    }
  }
}

const GOLDEN_TYPES = [[/^demo-sidebar/, 'SidebarView'], [/^demo-desk/, 'DeskView'], [/^demo-session/, 'SessionView'], [/^demo-card/, 'CardView'], [/^demo-notes/, 'NotesView'], [/^demo-settings/, 'SettingsView']]

// ---- the run ------------------------------------------------------------------------------------------------------------

const decls = declarations()
const files = [[TS_OUT, typescript(decls)], [SWIFT_OUT, swiftFile(decls)]]
if (process.argv.includes('--write')) for (const [file, text] of files) fs.writeFileSync(path.join(repo, file), text)
for (const [file, text] of files) {
  const now = fs.existsSync(path.join(repo, file)) ? read(file) : null
  if (now !== text) problem(`${file} is not what ${SOURCE} gives: run node tests/bindings/views.mjs --write`)
}
let golden = 0
for (const name of fs.readdirSync(path.join(repo, GOLDEN)).filter(n => n.endsWith('.json')).sort()) {
  const type = GOLDEN_TYPES.find(([re]) => re.test(name))?.[1]
  if (!type) { problem(`${GOLDEN}/${name}: which view is it? (add it to GOLDEN_TYPES)`); continue }
  check(JSON.parse(read(`${GOLDEN}/${name}`)), { k: 'named', name: type, args: [] }, name, decls)
  golden++
}
if (problems.length) {
  console.error(problems.slice(0, 40).join('\n'))
  process.exit(1)
}
console.log(`${decls.length} view types: TypeScript and Swift are current; ${golden} golden views read strictly`)
