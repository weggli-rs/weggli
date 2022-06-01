const CPP = require("../tree-sitter-cpp/grammar.js")

module.exports = grammar(CPP, {
  name: 'cpp',

  rules: {
    identifier: $ => /[\$a-zA-Z_]\w*/
  }
});
