export type CronFields = [string, string, string, string, string]

const CRON_FIELD_COUNT = 5

export function splitCronExpression(cronExpression: string): CronFields {
  const fields = cronExpression.trim().split(/\s+/)
  if (fields.length !== CRON_FIELD_COUNT) return ['', '', '', '', '']
  return [fields[0]!, fields[1]!, fields[2]!, fields[3]!, fields[4]!]
}

export function joinCronFields(cronFields: CronFields): string {
  return cronFields.map((field) => field.trim()).join(' ')
}

export function validCronFields(cronFields: CronFields): boolean {
  return cronFields.every((field) => field.trim().length > 0 && !/\s/.test(field.trim()))
}
