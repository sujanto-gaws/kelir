// The refusals (#777 AC5): what a load run will not run against.
//
// Checked before anything is started, read from the values the run is about to
// use, and all at once, so a refused run names everything wrong with it.

/** The stack on this host that holds real work. Never replaced by a load run. */
const REFUSED_PROJECTS = ['kelir-staging']
/** That stack's port. A load run never binds it and never sends to it. */
const REFUSED_PORTS = [8090]
/** The two databases that hold real work on this host. */
const REFUSED_DATABASES = ['kelir', 'kelir_dev']

/** Every reason this run must not start, or nothing. */
export function refusals({ project, ports, database }) {
  const reasons = []

  if (REFUSED_PROJECTS.includes(String(project).trim().toLowerCase())) {
    reasons.push(`the compose project is ${project}: a load run needs a project of its own`)
  }

  for (const port of ports) {
    if (REFUSED_PORTS.includes(Number(port))) {
      reasons.push(`port ${port} is the kelir-staging stack's`)
    }
  }

  if (REFUSED_DATABASES.includes(String(database).trim().toLowerCase())) {
    reasons.push(`the database is ${database}: a load run uses one made for the run`)
  }

  return reasons
}

/** The port a base URL addresses, for a smoke run pointed at another stack. */
export function portOf(url) {
  const parsed = new URL(url)

  return parsed.port || (parsed.protocol === 'https:' ? 443 : 80)
}
