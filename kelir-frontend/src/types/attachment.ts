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
  createdAt: string
  createdBy: string | null
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
