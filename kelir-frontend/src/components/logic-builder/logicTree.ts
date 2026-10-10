/**
 * The seam under the visual JSON Logic builder (#686, decision **D-110**).
 *
 * A JSON Logic value is parsed into a tree of nodes, each node is checked for
 * whether the builder can show it, and the tree is serialised back. **The unit
 * of representability is the node** (product owner, 2026-10-09): a node the
 * builder cannot show is an **opaque leaf** that holds the original value by
 * reference, and the rest of the tree around it stays editable.
 *
 * **Nothing is silently rewritten**, and the seam is where that is decided
 * rather than hoped for:
 *
 * - an opaque leaf serialises as the very value it was parsed from — the same
 *   reference, so no code path can re-sort its keys or re-order its array;
 * - a node the user did not touch serialises as its original value, also by
 *   reference, so an edit in one branch of `and` leaves its siblings untouched;
 * - a node the user did touch is rebuilt, and only the nodes on the path from
 *   it to the root are rebuilt with it.
 *
 * **What it offers is pinned** in {@link CALCULATE_TIER_OPERATORS} and
 * {@link CONDITIONAL_TIER_OPERATORS}: #686's section A, drawn from Calculation
 * Rule Registry §2.1 and §2.5. The registry approves more than the builder
 * offers — `min`, `max`, `map`, `if`, `in` and the rest — and those stay valid
 * expressions: the builder holds them opaque and the server's save-time check
 * (`CALCULATE_OPERATORS`, `CONDITIONAL_OPERATORS`) remains the only verdict on
 * them. **There is no second validator here.**
 *
 * Nothing in this module evaluates. Evaluation is `lib/jsonlogic.ts`'s
 * `loadEvaluator` (decision **D-10**), and this module does not import it.
 */

/** Which registry tier an expression is written in. */
export type LogicTier = 'calculate' | 'conditional'

/**
 * The operators the builder offers in a `calculate` expression.
 *
 * Calculation Rule Registry §2.1's arithmetic and `var`. §2.3 forbids
 * comparisons, `and`, `or` and `!` here, so a calculation containing one shows
 * it as an opaque leaf and lets the server refuse it at save.
 */
export const CALCULATE_TIER_OPERATORS = ['var', '+', '*', '-', '/', '%'] as const

/**
 * The operators the builder offers in a `conditional.logic` expression and in
 * every JWSS condition (registry §2.5, JWSS §6.2).
 *
 * Equality is four entries, never merged on parse: `===` and `==` evaluate
 * differently, and folding one into the other would rewrite an expression.
 */
export const CONDITIONAL_TIER_OPERATORS = [
  ...CALCULATE_TIER_OPERATORS,
  '==',
  '===',
  '!=',
  '!==',
  '>',
  '>=',
  '<',
  '<=',
  'and',
  'or',
  '!',
] as const

export type OfferedOperator = (typeof CONDITIONAL_TIER_OPERATORS)[number]

/** Every offered operator but `var`, which is a leaf rather than an operator node. */
export type ExpressionOperator = Exclude<OfferedOperator, 'var'>

export function offeredOperators(tier: LogicTier): readonly OfferedOperator[] {
  return tier === 'calculate' ? CALCULATE_TIER_OPERATORS : CONDITIONAL_TIER_OPERATORS
}

function isOffered(key: string, tier: LogicTier): key is OfferedOperator {
  return (offeredOperators(tier) as readonly string[]).includes(key)
}

/**
 * How many operands each operator takes, by section A.
 *
 * `-` and `/` are exactly two: one-operand `-` is negation and three or more is
 * a chain, and both are opaque rather than offered. The three-argument form of
 * `<` and `<=` is opaque for the same reason.
 */
export function operandCount(op: ExpressionOperator): { min: number; max: number } {
  switch (op) {
    case '+':
    case '*':
    case 'and':
    case 'or':
      return { min: 2, max: Number.POSITIVE_INFINITY }
    case '!':
      return { min: 1, max: 1 }
    default:
      return { min: 2, max: 2 }
  }
}

/** What the builder calls each operator. `===` and `!==` are the plain words, by section A. */
export const OPERATOR_LABELS: Record<ExpressionOperator, string> = {
  '+': 'plus',
  '*': 'times',
  '-': 'minus',
  '/': 'divided by',
  '%': 'remainder of',
  '===': 'is',
  '!==': 'is not',
  '==': 'equals (loose)',
  '!=': 'not equal (loose)',
  '>': 'greater than',
  '>=': 'at least',
  '<': 'less than',
  '<=': 'at most',
  and: 'all of',
  or: 'any of',
  '!': 'not',
}

/**
 * The operators grouped as the builder offers them. The first of each group is
 * what a new node of that group starts as — so a new comparison is `===`, "is",
 * by section A.
 */
export const OPERATOR_FAMILIES = {
  comparison: ['===', '!==', '==', '!=', '>', '>=', '<', '<='],
  arithmetic: ['+', '-', '*', '/', '%'],
  logical: ['and', 'or'],
  not: ['!'],
} as const satisfies Record<string, readonly ExpressionOperator[]>

export type OperatorFamily = keyof typeof OPERATOR_FAMILIES

export const FAMILY_LABELS: Record<OperatorFamily, string> = {
  comparison: 'Comparison',
  arithmetic: 'Arithmetic',
  logical: 'All of / any of',
  not: 'Not',
}

export function familyOf(op: ExpressionOperator): OperatorFamily {
  const families = Object.keys(OPERATOR_FAMILIES) as OperatorFamily[]

  return families.find((family) =>
    (OPERATOR_FAMILIES[family] as readonly string[]).includes(op),
  ) as OperatorFamily
}

/** The families a tier offers, in the order the builder lists them. */
export function familiesIn(tier: LogicTier): OperatorFamily[] {
  return (Object.keys(OPERATOR_FAMILIES) as OperatorFamily[]).filter((family) =>
    OPERATOR_FAMILIES[family].every((op) => isOffered(op, tier)),
  )
}

export type JsonLiteral = string | number | boolean | null

/** An operator the builder shows, with its operands. */
export interface OperatorNode {
  kind: 'operator'
  /** Identifies the slot across edits, so the view keeps its place. Not serialised. */
  key: number
  op: ExpressionOperator
  args: LogicNode[]
  /** `!` parsed as `{"!": x}` rather than `{"!": [x]}`; kept so it is emitted the same way. */
  bare: boolean
  /** The value this node was parsed from, until an edit inside it makes it stale. */
  original?: object
}

/** `{"var": "path"}`, the bare form only. */
export interface VarNode {
  kind: 'var'
  key: number
  path: string
  original?: object
}

export interface LiteralNode {
  kind: 'literal'
  key: number
  value: JsonLiteral
}

/** A value the builder cannot show, held by reference and emitted unchanged. */
export interface OpaqueNode {
  kind: 'opaque'
  key: number
  value: unknown
}

/**
 * What an unfilled hole is waiting for: the user has chosen the kind of
 * operand and not yet given it a value, or (`any`) not chosen the kind.
 */
export type HoleSlot = 'any' | 'var' | 'number' | 'string' | 'boolean' | 'json'

/**
 * An operand the user has not finished. **A tree containing one cannot be
 * serialised**: the builder never emits a placeholder.
 */
export interface HoleNode {
  kind: 'hole'
  key: number
  slot: HoleSlot
  /** Text typed so far that is not yet a value: a number being written, or JSON. */
  text?: string
}

export type LogicNode = OperatorNode | VarNode | LiteralNode | OpaqueNode | HoleNode

/** Indices into `args` from the root; `[]` is the root. */
export type NodePath = readonly number[]

/** A variable the host offers: a form's field keys, or a workflow's condition context. */
export interface LogicVariable {
  path: string
  label: string
  type?: string
}

/** What every node of one builder shares. */
export interface LogicBuilderContext {
  tier: LogicTier
  variables: readonly LogicVariable[]
  /** Whether a path outside `variables` may be typed. One already there is kept either way. */
  allowFreePaths: boolean
  /**
   * Prefixes under which every path counts as offered: an open-ended part of
   * the context the host cannot list, such as a workflow's `formData.`.
   */
  freePrefixes?: readonly string[]
  disabled: boolean
}

/** Thrown by {@link serialise} for a tree that still has a hole in it. */
export class IncompleteExpressionError extends Error {
  constructor() {
    super('The expression has an operand that is not filled in.')
    this.name = 'IncompleteExpressionError'
  }
}

let lastKey = 0

function nextKey(): number {
  lastKey += 1

  return lastKey
}

function isPlainObject(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

function opaque(value: unknown): OpaqueNode {
  return { kind: 'opaque', key: nextKey(), value }
}

/**
 * How deep the builder shows an expression, counting the root as depth 0.
 *
 * Anything but a literal nested deeper is held as one opaque leaf, by
 * reference, so the round trip stays exact and nothing below it is visited.
 * The bound is the view's, not the seam's. The recursive parse overflows
 * somewhere past 2,000 levels; Vue's nested render overflows far sooner, and
 * soonest **cold**, on a page whose first render is the deep expression: from
 * about 248 levels in a fresh spec file, against 300–400 once earlier tests
 * have warmed the render path (measured 2026-10-09 in vitest, jsdom on Node's
 * default stack). A browser's stack is not Node's, and no browser test covers
 * this, so the cap sits well clear of the cold limit rather than near it. An
 * expression a person writes is a handful of levels deep.
 */
export const MAX_VISUAL_DEPTH = 64

/** An operand: a bare or nested array there is opaque (B6). */
function parseOperand(value: unknown, tier: LogicTier, depth: number): LogicNode {
  return Array.isArray(value) ? opaque(value) : parseNode(value, tier, depth)
}

function parseNode(value: unknown, tier: LogicTier, depth: number): LogicNode {
  if (
    value === null ||
    typeof value === 'string' ||
    typeof value === 'boolean' ||
    typeof value === 'number'
  ) {
    return { kind: 'literal', key: nextKey(), value }
  }

  if (depth > MAX_VISUAL_DEPTH || !isPlainObject(value)) {
    return opaque(value)
  }

  const keys = Object.keys(value)

  // B7: an object of two keys or none asserts no meaning the builder could show.
  if (keys.length !== 1) {
    return opaque(value)
  }

  const op = keys[0]

  // B3: unknown to the registry, approved but not offered, or forbidden in this
  // tier — all three are opaque, and none is a verdict.
  if (!isOffered(op, tier)) {
    return opaque(value)
  }

  const argument = value[op]

  if (op === 'var') {
    // B4: a non-empty string path. An array, a default, a number or `""` is opaque.
    return typeof argument === 'string' && argument !== ''
      ? { kind: 'var', key: nextKey(), path: argument, original: value }
      : opaque(value)
  }

  if (op === '!' && !Array.isArray(argument)) {
    return {
      kind: 'operator',
      key: nextKey(),
      op,
      args: [parseOperand(argument, tier, depth + 1)],
      bare: true,
      original: value,
    }
  }

  if (!Array.isArray(argument)) {
    return opaque(value)
  }

  const { min, max } = operandCount(op)

  if (argument.length < min || argument.length > max) {
    return opaque(value)
  }

  return {
    kind: 'operator',
    key: nextKey(),
    op,
    args: argument.map((operand) => parseOperand(operand, tier, depth + 1)),
    bare: false,
    original: value,
  }
}

/**
 * Parses a JSON Logic value into a tree. Never throws: what it cannot show is
 * opaque, and so is anything nested past {@link MAX_VISUAL_DEPTH}.
 */
export function parseExpression(value: unknown, tier: LogicTier): LogicNode {
  return parseNode(value, tier, 0)
}

/**
 * Whether the builder can show this tree at all (B2).
 *
 * The root must be an operator object it offers, `var` included. A bare
 * literal, an array, `null`, `{}`, a multi-key object or an operator it does
 * not offer opens the whole expression in the raw-JSON box instead.
 */
export function isRootRepresentable(node: LogicNode): boolean {
  return node.kind === 'operator' || node.kind === 'var'
}

/** Whether every operand is filled in, so the tree can be emitted. */
export function isComplete(node: LogicNode): boolean {
  if (node.kind === 'hole') {
    return false
  }

  return node.kind !== 'operator' || node.args.every(isComplete)
}

/** Whether any node in the tree is opaque. */
export function containsOpaque(node: LogicNode): boolean {
  if (node.kind === 'opaque') {
    return true
  }

  return node.kind === 'operator' && node.args.some(containsOpaque)
}

function emit(node: LogicNode, useOriginals: boolean): unknown {
  switch (node.kind) {
    case 'opaque':
      return node.value
    case 'literal':
      return node.value
    case 'hole':
      throw new IncompleteExpressionError()
    case 'var':
      return useOriginals && node.original ? node.original : { var: node.path }
    case 'operator': {
      if (useOriginals && node.original) {
        return node.original
      }

      const operands = node.args.map((arg) => emit(arg, useOriginals))

      return { [node.op]: node.bare ? operands[0] : operands }
    }
  }
}

/**
 * The value the builder emits for this tree.
 *
 * An untouched node is its original value and an opaque leaf is its held
 * value, both by reference; only nodes an edit made stale are built afresh.
 *
 * @throws IncompleteExpressionError when a hole is left in the tree.
 */
export function serialise(node: LogicNode): unknown {
  return emit(node, true)
}

/**
 * The value this tree builds when no node's original is used.
 *
 * What {@link serialise} does for an edited node, applied to every node. The
 * builder never needs it; the round-trip test does, because it is what proves
 * the tree itself loses nothing — `serialise` of an untouched tree is the
 * input by construction. Opaque leaves are still emitted by reference.
 */
export function rebuild(node: LogicNode): unknown {
  return emit(node, false)
}

/**
 * Whether a `var` path is one the host offers: a listed variable, or a path
 * under one of `freePrefixes` with something after the prefix. Anything else
 * is kept, with a warning (B4).
 */
export function isOfferedPath(
  path: string,
  variables: readonly LogicVariable[],
  freePrefixes: readonly string[] = [],
): boolean {
  return (
    variables.some((variable) => variable.path === path) ||
    freePrefixes.some((prefix) => path.length > prefix.length && path.startsWith(prefix))
  )
}

export function literalNode(value: JsonLiteral): LiteralNode {
  return { kind: 'literal', key: nextKey(), value }
}

export function varNode(path: string): VarNode {
  return { kind: 'var', key: nextKey(), path }
}

export function holeNode(slot: HoleSlot, text?: string): HoleNode {
  return text === undefined
    ? { kind: 'hole', key: nextKey(), slot }
    : { kind: 'hole', key: nextKey(), slot, text }
}

export function opaqueNode(value: unknown): OpaqueNode {
  return opaque(value)
}

/** A new operator with every operand unfilled. */
export function operatorNode(op: ExpressionOperator): OperatorNode {
  const { min } = operandCount(op)

  return {
    kind: 'operator',
    key: nextKey(),
    op,
    args: Array.from({ length: min }, () => holeNode('any')),
    bare: false,
  }
}

/** What the user can choose an operand to be. A family makes its first operator. */
export type NodeChoice = 'var' | 'number' | 'string' | 'boolean' | 'null' | 'json' | OperatorFamily

/**
 * A fresh node of the chosen kind. Only `null` is complete on creation: every
 * other kind waits for the user to give it a value, and none is given a
 * placeholder one.
 */
export function createNode(choice: NodeChoice): LogicNode {
  switch (choice) {
    case 'null':
      return literalNode(null)
    case 'var':
    case 'number':
    case 'string':
    case 'boolean':
    case 'json':
      return holeNode(choice)
    default:
      return operatorNode(OPERATOR_FAMILIES[choice][0])
  }
}

/** The node at `path`. */
export function nodeAt(root: LogicNode, path: NodePath): LogicNode {
  let node = root

  for (const at of path) {
    if (node.kind !== 'operator' || at < 0 || at >= node.args.length) {
      throw new RangeError(`No operand at ${path.join('.')}`)
    }

    node = node.args[at]
  }

  return node
}

/**
 * Rewrites the operator at `path` with `change`, and every ancestor on the way
 * up as stale. Siblings off the path are the same objects, so they keep their
 * originals — which is what makes an edit change only the edited branch.
 */
function updateOperatorAt(
  root: LogicNode,
  path: NodePath,
  change: (node: OperatorNode) => OperatorNode,
): LogicNode {
  if (path.length === 0) {
    if (root.kind !== 'operator') {
      throw new RangeError('Not an operator')
    }

    return change(root)
  }

  if (root.kind !== 'operator') {
    throw new RangeError(`No operand at ${path.join('.')}`)
  }

  const [at, ...rest] = path

  if (at < 0 || at >= root.args.length) {
    throw new RangeError(`No operand at ${path.join('.')}`)
  }

  const args = root.args.slice()
  args[at] = updateOperatorAt(root.args[at], rest, change)

  return { ...root, args, original: undefined }
}

/** Replaces the node at `path`. The replacement takes the old node's slot key. */
export function replaceAt(root: LogicNode, path: NodePath, next: LogicNode): LogicNode {
  const placed = { ...next, key: nodeAt(root, path).key }

  if (path.length === 0) {
    return placed
  }

  const parentPath = path.slice(0, -1)
  const at = path[path.length - 1]

  return updateOperatorAt(root, parentPath, (parent) => {
    const args = parent.args.slice()
    args[at] = placed

    return { ...parent, args, original: undefined }
  })
}

/** Removes the operand at `path`, which its operator must be able to spare. */
export function removeOperandAt(root: LogicNode, path: NodePath): LogicNode {
  if (path.length === 0) {
    throw new RangeError('The root is not an operand')
  }

  const at = path[path.length - 1]

  return updateOperatorAt(root, path.slice(0, -1), (parent) => {
    if (parent.args.length <= operandCount(parent.op).min) {
      throw new RangeError(`${parent.op} needs ${operandCount(parent.op).min} operands`)
    }

    return { ...parent, args: parent.args.filter((_, index) => index !== at), original: undefined }
  })
}

/** Appends an unfilled operand to the operator at `path`. */
export function addOperandAt(root: LogicNode, path: NodePath): LogicNode {
  return updateOperatorAt(root, path, (parent) => {
    if (parent.args.length >= operandCount(parent.op).max) {
      throw new RangeError(`${parent.op} takes no more operands`)
    }

    return { ...parent, args: [...parent.args, holeNode('any')], original: undefined }
  })
}

/** One user edit, as the view reports it. */
export type LogicEdit =
  | { type: 'replace'; path: NodePath; node: LogicNode }
  | { type: 'remove'; path: NodePath }
  | { type: 'add'; path: NodePath }
  | { type: 'operator'; path: NodePath; op: ExpressionOperator }

/** Applies one edit, returning a new tree. The tree passed in is not changed. */
export function applyEdit(root: LogicNode, edit: LogicEdit): LogicNode {
  switch (edit.type) {
    case 'replace':
      return replaceAt(root, edit.path, edit.node)
    case 'remove':
      return removeOperandAt(root, edit.path)
    case 'add':
      return addOperandAt(root, edit.path)
    case 'operator':
      return changeOperatorAt(root, edit.path, edit.op)
  }
}

/** Whether `op` can replace this operator without dropping an operand. */
export function canChangeOperator(node: OperatorNode, op: ExpressionOperator): boolean {
  return node.args.length <= operandCount(op).max
}

/**
 * Changes the operator at `path`, keeping its operands. One the new operator
 * needs and the old one did not have is added unfilled; one it cannot take is
 * refused rather than dropped.
 */
export function changeOperatorAt(
  root: LogicNode,
  path: NodePath,
  op: ExpressionOperator,
): LogicNode {
  return updateOperatorAt(root, path, (node) => {
    if (!canChangeOperator(node, op)) {
      throw new RangeError(`${op} cannot take ${node.args.length} operands`)
    }

    const { min } = operandCount(op)
    const args = node.args.slice()

    while (args.length < min) {
      args.push(holeNode('any'))
    }

    return { ...node, op, args, bare: op === '!' && node.bare, original: undefined }
  })
}

/** A short reading of a node, for a collapsed branch. */
export function summarise(node: LogicNode): string {
  switch (node.kind) {
    case 'var':
      return node.path
    case 'literal':
      return JSON.stringify(node.value)
    case 'opaque':
      return 'advanced'
    case 'hole':
      return '?'
    case 'operator':
      return node.op === '!'
        ? `not ${summarise(node.args[0])}`
        : `(${node.args.map(summarise).join(` ${node.op} `)})`
  }
}
