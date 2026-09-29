import type { Page } from '@/types/api'

/**
 * How many rows a chooser asks for: the most any list endpoint returns in one
 * page (`pageSize` is clamped to 1..=100 on the server). A chooser shows one
 * page and searches for the rest; it never pages.
 */
export const SEARCH_SELECT_PAGE_SIZE = 100

/** What `SearchSelect` asks its source for: one page, narrowed by the search. */
export interface SearchSelectQuery {
  /** Trimmed. Blank means "no filter", and the API clients leave it off the wire. */
  search: string
  pageSize: number
}

/**
 * Where a chooser's rows come from and how one row reads as a choice.
 *
 * One object rather than six props (coding standard §3.2): these travel
 * together, and a chooser is described by them.
 */
export interface SearchSource<Row> {
  /**
   * One page of rows matching the search. **Every filter that is a column
   * belongs here, on the server** (`status: 'ACTIVE'`): a page filtered after
   * it arrives shows fewer than a page and hides whatever sorted past it (#525).
   */
  fetch: (query: SearchSelectQuery) => Promise<Page<Row>>
  value: (row: Row) => string
  label: (row: Row) => string
  /** A row shown but not choosable, where the screen explains why. */
  disabled?: (row: Row) => boolean
  /**
   * A row left out on the client, for a rule the server cannot be asked — "not
   * yourself". Not for status: that is `fetch`'s.
   */
  exclude?: (row: Row) => boolean
  /**
   * Reads one row by its value, to label a stored choice that no search has
   * returned yet — a type bound to a form that sorts past the first page.
   */
  resolve?: (value: string) => Promise<Row>
}

/** A label the caller already holds for a value, such as the roles a user record names. */
export interface KnownOption {
  value: string
  label: string
}
