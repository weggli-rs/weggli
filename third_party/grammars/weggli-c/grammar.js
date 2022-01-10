const C = require("../tree-sitter-c/grammar.js")

module.exports = grammar(C, {
  name: 'c',

  rules: {
    identifier: $ => /[\$a-zA-Z_]\w*/,

  }
});