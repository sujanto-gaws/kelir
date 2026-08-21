const cases = [
  ['plain', String.raw`^[A-Z]{3}-\d{4}$`, 'ABC-1234'],
  ['case-insensitive via inline flag', '^abc$', 'ABC', 'i'],
  ['lookahead (password complexity)', String.raw`^(?=.*[A-Z])(?=.*\d).{8,}$`, 'Passw0rdd'],
  ['backreference (repeated token)', String.raw`^(\w+)-\1$`, 'ab-ab'],
  ['ASCII vs Unicode digit class', String.raw`^\d+$`, '٣٤٥'],
  ['dollar and multiline', '^a$', 'a\n'],
];
for (const [label, pattern, input, flags] of cases) {
  try {
    console.log(label.padEnd(34), 'COMPILES   matches =', new RegExp(pattern, flags || '').test(input));
  } catch (error) {
    console.log(label.padEnd(34), 'REJECTED  ', error.message);
  }
}
