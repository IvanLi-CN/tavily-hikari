// Commitlint configuration enforcing conventional commits and English-only messages.

module.exports = {
  extends: ['@commitlint/config-conventional'],
  plugins: ['commitlint-plugin-function-rules'],
  rules: {
    'type-enum': [
      2,
      'always',
      ['feat', 'fix', 'docs', 'style', 'refactor', 'perf', 'test', 'chore', 'ci', 'build', 'revert'],
    ],
    'type-case': [2, 'always', 'lower-case'],
    'type-empty': [2, 'never'],
    'scope-case': [2, 'always', 'lower-case'],
    'subject-empty': [2, 'never'],
    'subject-full-stop': [2, 'never', '.'],
    'subject-case': [0],
    'function-rules/subject-case': [
      2,
      'always',
      (parsed) => {
        const { subject } = parsed;
        if (!subject) return [true];

        const chineseRegex = /[\u4e00-\u9fff]/;
        if (chineseRegex.test(subject)) {
          return [false, 'Subject must be in English only. Chinese characters are not allowed.'];
        }

        if (/^[A-Z]/.test(subject)) {
          return [false, 'Subject must not start with uppercase'];
        }

        return [true];
      },
    ],
    'header-max-length': [2, 'always', 72],
    'body-empty': [1, 'never'],
    'body-leading-blank': [2, 'always'],
    'body-max-line-length': [2, 'always', 100],
    'body-case': [0],
    'function-rules/body-case': [
      2,
      'always',
      (parsed) => {
        const { body } = parsed;
        if (!body) return [true];

        const chineseRegex = /[\u4e00-\u9fff]/;
        if (chineseRegex.test(body)) {
          return [false, 'Body must be in English only. Chinese characters are not allowed.'];
        }

        return [true];
      },
    ],
    'footer-leading-blank': [1, 'always'],
    'footer-max-line-length': [2, 'always', 100],
  },
};
