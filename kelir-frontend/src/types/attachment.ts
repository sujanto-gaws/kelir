/**
 * Attachments, as the API reports them (`/api/v1/documents/{id}/attachments`).
 *
 * The server's shape, unchanged: what a screen adds is in
 * `features/attachments`, not here.
 */

/**
 * How far a file has got through the scanner.
 *
 * **Only `CLEAN` is downloadable**, and the other three are refusals rather
 * than stages of one (#246). The distinction matters on a screen more than
 * anywhere else, because *not yet* and *never* need different things from the
 * person looking at them.
 */
export type VirusScanStatus = 'PENDING' | 'CLEAN' | 'INFECTED' | 'FAILED'

/** One category a file or a link can be filed under (FR-ATT-006). */
export interface AttachmentCategory {
  id: string
  code: string
  name: string
  /** Seeded by this product. A tenant may use it and may not delete it. */
  isSystem: boolean
}

export interface Attachment {
  id: string
  documentId: string
  /** The name as uploaded, which is the one the person recognises. */
  originalFileName: string
  mimeType: string
  fileSize: number
  checksum: string
  description: string | null
  virusScanStatus: VirusScanStatus
  /** What kind of thing it is, or null on a file nobody has filed. */
  category: AttachmentCategory | null
  createdAt: string
  createdBy: string | null
}

/**
 * A link to something this product does not hold (FR-ATT-010).
 *
 * **It is deliberately not an `Attachment`, and the type is the argument.**
 * There is no `fileSize`, no `mimeType` and no `virusScanStatus` here because
 * there is none on the row: a reference is not a file, is never scanned, and can
 * never read `CLEAN` (#254 AC4, AC5). A screen holding one of these cannot
 * render a size or a scan badge for it by accident, which is the half of *visibly
 * not a file* that a shared type would have left to a convention.
 */
export interface ExternalReference {
  id: string
  documentId: string
  /** What to call the link — never the URL, which is where a lookalike hides. */
  label: string
  url: string
  description: string | null
  category: AttachmentCategory | null
  createdAt: string
  createdBy: string | null
}

/**
 * An `href` this screen is willing to put in the page.
 *
 * **The server already refuses anything but http and https**
 * (`attachment::domain::normalize_url`), so this is defence in depth rather than
 * the gate — and it is worth having anyway: a row that predates the check, or
 * one written by a surface somebody adds later, would otherwise reach an
 * `href` where `javascript:` is script execution in this product's page with
 * this product's session. A refused link renders as text and goes nowhere.
 */
export function safeHref(url: string): string | undefined {
  const trimmed = url.trim().toLowerCase()

  return trimmed.startsWith('http://') || trimmed.startsWith('https://') ? url : undefined
}

/**
 * What the badge says.
 *
 * Typed as `Record<VirusScanStatus, string>` so a status added to the API
 * cannot render blank — the same guard `TASK_STATUS_LABELS` uses one feature
 * over, and the reason record 09 §6.4 checked it there.
 */
export const SCAN_STATUS_LABELS: Record<VirusScanStatus, string> = {
  PENDING: 'Checking',
  CLEAN: 'Ready',
  INFECTED: 'Infected',
  FAILED: 'Not checked',
}

/**
 * Why a file cannot be downloaded, in the words the person needs.
 *
 * **Three messages, not one** (#246 AC3, and #295's own second criterion). A
 * screen that renders one spinner for all three turns a security control into a
 * bug report:
 *
 * * `PENDING` is waiting, and will resolve on its own.
 * * `INFECTED` will never resolve, and the file needs replacing.
 * * `FAILED` will never resolve either, and **is not an error in this product**
 *   — a scan that could not run has cleared nothing, so the file is refused
 *   exactly as an infected one is, and the person should upload it again.
 */
export const SCAN_STATUS_EXPLANATIONS: Record<VirusScanStatus, string> = {
  PENDING: 'Being checked for viruses. It will be available to download shortly.',
  CLEAN: 'Checked and available.',
  INFECTED:
    'A virus was found in this file, so it will not be served. Remove it and upload a clean copy.',
  FAILED:
    'This file could not be checked, so it will not be served — a check that did not run has cleared nothing. Upload it again.',
}

/** Whether the bytes can be served (#246 AC2). */
export function isDownloadable(status: VirusScanStatus): boolean {
  return status === 'CLEAN'
}
