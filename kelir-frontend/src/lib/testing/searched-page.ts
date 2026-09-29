import type { FakeReply, RecordedRequest } from './fake-backend'

/**
 * Test support — never imported by application code.
 *
 * Answers a list request the way a list endpoint that searches does (#525):
 * `search` is a case-insensitive substring match on `fields`, `status` an exact
 * match on the row's `status`, and the reply is the first `pageSize` (at most
 * 100) of what matched, with `meta.total` counting all of it. Rows are answered
 * in the order given, which a caller builds in the endpoint's own key order.
 *
 * A fake that ignored the parameters would let a chooser that filtered on the
 * client pass; this one only finds what the server would.
 */
export function searchedPage<Row extends object>(
  request: RecordedRequest,
  rows: Row[],
  fields: (keyof Row & string)[],
): FakeReply {
  const search = String(request.params.search ?? '').toLowerCase()
  const status = request.params.status as string | undefined
  const pageSize = Math.min(Number(request.params.pageSize ?? 20), 100)

  const matching = rows.filter((row) => {
    const record = row as Record<string, unknown>

    if (status !== undefined && record.status !== status) {
      return false
    }

    return (
      search === '' ||
      fields.some((field) =>
        String(record[field] ?? '')
          .toLowerCase()
          .includes(search),
      )
    )
  })

  return {
    status: 200,
    body: {
      success: true,
      data: matching.slice(0, pageSize),
      meta: { page: 1, pageSize, total: matching.length },
    },
  }
}

/**
 * `count` rows made by `make`, numbered from 1 and zero-padded so that they
 * sort by their number — the filler a chooser test puts ahead of the row it
 * wants, so that row sorts past the first page.
 */
export function filler<Row>(count: number, make: (n: string) => Row): Row[] {
  return Array.from({ length: count }, (_, index) => make(String(index + 1).padStart(3, '0')))
}
