#include <tree_sitter/parser.h>

#include <assert.h>
// #include <printf.h>  // for debugging
#include <stdlib.h>
#include <string.h>
#include <ctype.h>
#include <wctype.h>

// Define the maximum string length
#define MAX_STRING_LENGTH 1000

// #define MAX(a, b) ((a) > (b) ? (a) : (b))

// #define MIN(a, b) ((a) < (b) ? (a) : (b))

// // String related macros
// #define STRING_RESIZE(vec, _cap)                                         \
//   void *tmp = realloc((vec).data, ((_cap) + 1) * sizeof((vec).data[0])); \
//   assert(tmp != NULL);                                                   \
//   (vec).data = tmp;                                                      \
//   memset((vec).data + (vec).len, 0,                                      \
//          (((_cap) + 1) - (vec).len) * sizeof((vec).data[0]));            \
//   (vec).cap = (_cap);

// #define STRING_GROW(vec, _cap)    \
//   if ((vec).cap < (_cap)) {       \
//     STRING_RESIZE((vec), (_cap)); \
//   }

// #define STRING_PUSH(vec, el)                      \
//   if ((vec).cap == (vec).len) {                   \
//     STRING_RESIZE((vec), MAX(16, (vec).len * 2)); \
//   }                                               \
//   (vec).data[(vec).len++] = (el);

// #define STRING_FREE(vec)  \
//   if ((vec).data != NULL) \
//     free((vec).data);     \
//   (vec).data = NULL;

// #define STRING_CLEAR(vec)                            \
//   {                                                  \
//     (vec).len = 0;                                   \
//     memset((vec).data, 0, (vec).cap * sizeof(char)); \
//   }

enum TokenType {
  SCALAR_VARIABLE_EXTERNAL,
  START_DELIMITER,
  END_DELIMITER,
  STRING_CONTENT,
  STRING_SINGLE_QUOTED_CONTENT,
  STRING_SINGLE_Q_QUOTED_CONTENT,
  STRING_QQ_QUOTED_CONTENT,
  STRING_DOUBLE_QUOTED_CONTENT,
  START_DELIMITER_QW,
  ELEMENT_IN_QW,
  END_DELIMITER_QW,
  START_DELIMITER_REGEX,
  START_DELIMITER_SLASH,
  REGEX_PATTERN,
  END_DELIMITER_REGEX,
  START_DELIMITER_SEARCH_REPLACE,
  SEARCH_REPLACE_CONTENT,
  SEPARATOR_DELIMITER_SEARCH_REPLACE,
  END_DELIMITER_SEARCH_REPLACE,
  START_DELIMITER_TRANSLITERATION,
  TRANSLITERATION_CONTENT,
  SEPARATOR_DELIMITER_TRANSLITERATION,
  END_DELIMITER_TRANSLITERATION,
  IMAGINARY_HEREDOC_START,
  HEREDOC_START_IDENTIFIER,
  HEREDOC_CONTENT,
  HEREDOC_END_IDENTIFIER,
  POD_CONTENT,
  DATA_NOT_FOR_COMPILER,
  AUTOMATIC_SEMICOLON,
};

// typedef struct
// {
//   uint32_t cap;
//   uint32_t len;
//   char *data;
// } String;

// static String string_new() {
//   return (String){.cap = 16, .len = 0, .data = calloc(1, sizeof(char) * 17)};
// }

// static char *my_strdup(const char *str) {
//   // Calculate the length of the input string
//   size_t len = strlen(str) + 1; // Include space for the null terminator
  
//   // Allocate memory for the duplicated string
//   char *duplicate = (char *)calloc(len, len * sizeof(char));
//   if (duplicate == NULL) {
//       return NULL; // Return NULL if memory allocation failed
//   }
  
//   // Copy the input string to the newly allocated memory using strncpy
//   strncpy(duplicate, str, len); // Copy up to 'len' characters from 'str' to 'duplicate'
//   duplicate[len - 1] = '\0'; // Ensure null-termination of the duplicated string
  
//   return duplicate; // Return a pointer to the duplicated string
// }

// START OF --- common implementation of queue in C
// Enum to represent data types
typedef enum {
    STRING,
    BOOLEAN
} DataType;

// Union to represent data
typedef union {
    char string[MAX_STRING_LENGTH];
    bool boolean;
} Data;

// Structure to represent a queue node
typedef struct Node {
    Data data;
    DataType type;
    struct Node* next;
} Node;

// Structure to represent the queue
typedef struct {
    Node* front;
    Node* rear;
} Queue;

// Function to create a new queue node
static Node* create_node(Data data, DataType type) {
    Node* node = (Node*) malloc(sizeof(Node));
    node->data = data;
    node->type = type;
    node->next = NULL;
    return node;
}

// Function to create a new queue
static Queue* create_queue() {
    Queue* queue = (Queue*) malloc(sizeof(Queue));
    queue->front = NULL;
    queue->rear = NULL;
    return queue;
}

// Function to check if the queue is empty
static int is_empty(Queue* queue) {
    return queue->front == NULL;
}

// Function to add data to the end of the queue
static void enqueue(Queue* queue, Data data, DataType type) {
    Node* node = create_node(data, type);
    if (is_empty(queue)) {
        queue->front = node;
        queue->rear = node;
    } else {
        queue->rear->next = node;
        queue->rear = node;
    }
}

// Function to remove data from the front of the queue
static Data dequeue(Queue* queue) {
    if (is_empty(queue)) {
        Data empty_data;
        return empty_data;
    }
    Data data = queue->front->data;
    Node* temp = queue->front;
    queue->front = queue->front->next;
    if (queue->front == NULL) {
        queue->rear = NULL;
    }
    free(temp);
    return data;
}

// Function to return the data at the front of the queue without removing it
static Data peek(Queue* queue) {
    if (is_empty(queue)) {
        Data empty_data;
        return empty_data;
    }
    return queue->front->data;
}

// END OF --- common implementation of queue in C

// void string_push(char source[], ) {}


// START OF --- a array implementation of STRING queue in C
// typedef struct
// {
//   int front, rear, size;
//   int capacity;
//   char **data;
// } StringQueue;

// // function to create a queue
// // of given capacity.
// // It initializes size of queue as 0
// static StringQueue *createStringQueue() {
//   StringQueue *queue = calloc(1, sizeof(StringQueue));

//   queue->capacity = 1;
//   queue->front = queue->size = 0;

//   queue->rear = queue->capacity - 1;
//   queue->data = (char **)calloc(1, queue->capacity * sizeof(char));
//   return queue;
// }

// // StringQueue is full when size becomes
// // equal to the capacity
// static int isStringQueueFull(StringQueue *queue) {
//   return (queue->size == queue->capacity);
// }

// // StringQueue is empty when size is 0
// static int isStringQueueEmpty(StringQueue *queue) {
//   return (queue->size == 0);
// }

// // Function to add an item to the queue.
// // It changes rear and size
// static void enqueueStringQueue(StringQueue *queue, String *item) {
//   if (isStringQueueFull(queue))
//     queue->capacity = queue->capacity + 1;;
//   queue->rear = (queue->rear + 1) % queue->capacity;
//   queue->data[queue->rear] = my_strdup(item->data);
//   queue->size = queue->size + 1;
// }

// // Function to remove an item from queue.
// // It changes front and size
// static char *dequeueStringQueue(StringQueue *queue) {
//   if (isStringQueueEmpty(queue))
//     return NULL;
//   char *item = queue->data[queue->front];
//   queue->front = (queue->front + 1) % queue->capacity;
//   queue->size = queue->size - 1;
//   queue->capacity = queue->capacity - 1;
//   return item;
// }

// // Function to get front of queue
// static char *front(StringQueue *queue) {
//   // if (isQueueEmpty(queue))
//   //   return CHAR_MIN;
//   // return queue->data[queue->front];
//   if (queue->data[queue->front]) {
//     return queue->data[queue->front];
//   }
//   return NULL;
// }

// void deleteStringQueue(StringQueue *queue) {
//   if (queue != NULL) {
//     // Free the dynamically allocated data array
//     free(queue->data);
//     // Free the BoolQueue structure itself
//     free(queue);
//   }
// }

// // END OF --- a array implementation of STRING queue in C

// // START OF --- a array implementation of Boolean queue in C
// typedef struct
// {
//   int front, rear, size;
//   int capacity;
//   bool *data;
// } BoolQueue;

// // function to create a queue
// // of given capacity.
// // It initializes size of queue as 0
// static BoolQueue *createBoolQueue() {
//   BoolQueue *queue = calloc(1, sizeof(BoolQueue));

//   queue->capacity = 1;
//   queue->front = queue->size = 0;

//   // This is important, see the enqueue
//   queue->rear = queue->capacity - 1;
//   queue->data = (bool *)calloc(1, queue->capacity * sizeof(bool));
//   return queue;
// }

// // BoolQueue is full when size becomes
// // equal to the capacity
// static int isBoolQueueFull(BoolQueue *queue) {
//   return (queue->size == queue->capacity);
// }

// // BoolQueue is empty when size is 0
// static int isBoolQueueEmpty(BoolQueue *queue) {
//   return (queue->size == 0);
// }

// // Function to add an item to the queue.
// // It changes rear and size
// static void enqueueBoolQueue(BoolQueue *queue, bool item) {
//   if (isBoolQueueFull(queue))
//     queue->capacity = queue->capacity + 1;;
//   queue->rear = (queue->rear + 1) % queue->capacity;
//   queue->data[queue->rear] = item;
//   queue->size = queue->size + 1;
// }

// // Function to remove an item from queue.
// // It changes front and size
// static bool dequeueBoolQueue(BoolQueue *queue) {
//   if (isBoolQueueEmpty(queue))
//     return false;
//   bool item = queue->data[queue->front];
//   queue->front = (queue->front + 1) % queue->capacity;
//   queue->size = queue->size - 1;
//   return item;
// }

// // Function to get front of queue
// static bool frontBoolQueue(BoolQueue *queue) {
//   // if (isBoolQueueEmpty(queue))
//   //   return CHAR_MIN;
//   if (queue->data[queue->front]) {
//     return queue->data[queue->front];
//   }
//   return NULL;
// }

// void deleteBoolQueue(BoolQueue *queue) {
//   if (queue != NULL) {
//     // Free the dynamically allocated data array
//     free(queue->data);
//     // Free the BoolQueue structure itself
//     free(queue);
//   }
// }

// // END OF --- a array implementation of Boolean queue in C

typedef struct {
  bool started_heredoc;
  bool started_heredoc_body;
  Queue *heredoc_identifier_queue;
  Queue *heredoc_allows_interpolation;
  Queue *heredoc_allows_indent;
} Heredoc;

static Heredoc heredoc_new() {
  Heredoc heredoc = {
      .started_heredoc = false,
      .started_heredoc_body = false,
      .heredoc_identifier_queue = create_queue(),
      .heredoc_allows_interpolation = create_queue(),
      .heredoc_allows_indent = create_queue(),
  };
  return heredoc;
}

typedef struct
{
  int32_t start_delimiter_char;
  int32_t end_delimiter_char;
  bool is_separator_delimiter_parsed;
  bool is_delimiter_enclosing;  // is the delimiter {}, <> and same character not //, !!
  Heredoc heredoc;
} Scanner;

static void advance(TSLexer *lexer) {
  lexer->advance(lexer, false);
}

static void skip(TSLexer *lexer) {
  lexer->advance(lexer, true);
}

// runs over spaces like a champ
static void run_over_spaces(TSLexer *lexer) {
  while (iswspace(lexer->lookahead))
    skip(lexer);
}

// runs with the spaces using advance
static void run_with_spaces(TSLexer *lexer) {
  while (iswspace(lexer->lookahead))
    advance(lexer);
}

static bool is_identifier(TSLexer *lexer) {
  return (iswalnum(lexer->lookahead) || lexer->lookahead == '_');
}

static int32_t get_end_delimiter(Scanner *scanner) {
  return scanner->end_delimiter_char;
}

static bool handle_interpolation(Scanner *scanner, TSLexer *lexer, enum TokenType surrounding_token) {
  if (lexer->lookahead == '$') {
    // allow $ to be last character in a regex
    if (surrounding_token == SEARCH_REPLACE_CONTENT || surrounding_token == REGEX_PATTERN) {
      advance(lexer);
      run_with_spaces(lexer);
      if (lexer->lookahead == get_end_delimiter(scanner)) {
        lexer->result_symbol = surrounding_token;
        lexer->mark_end(lexer);
        return true;
      }
    }
    return false;
  }

  return false;
}

static bool handle_escape_sequence(TSLexer *lexer, enum TokenType surrounding_token) {
  // escape sequences, only basic support as of now
  if (lexer->lookahead == '\\') {
    advance(lexer);
    // also, self end delimiter will be treated as string
    if (
        lexer->lookahead == 't' || lexer->lookahead == 'n' || lexer->lookahead == 'r' || lexer->lookahead == 'f' || lexer->lookahead == 'b' || lexer->lookahead == 'a' || lexer->lookahead == 'e') {
      // advance(lexer);
      lexer->mark_end(lexer);
      return false;
    } else {
      lexer->result_symbol = surrounding_token;
      advance(lexer);
      lexer->mark_end(lexer);
      return true;
    }
    return false;
  }
  return false;
}

static bool scan_nested_delimiters(Scanner *scanner, TSLexer *lexer, enum TokenType token_type) {
  while (lexer->lookahead) {
    if (lexer->lookahead == get_end_delimiter(scanner)) {
      lexer->result_symbol = token_type;
      advance(lexer);
      lexer->mark_end(lexer);
      return true;
    } else if (lexer->lookahead == scanner->start_delimiter_char) {
      lexer->result_symbol = token_type;
      advance(lexer);
      scan_nested_delimiters(scanner, lexer, token_type);
    } else if (lexer->lookahead == '\\') {
      advance(lexer);
      advance(lexer);
    } else {
      advance(lexer);
    }
  }
  lexer->mark_end(lexer);
  return false;
}

static bool parse_delimited_and_interpolated_content(Scanner *scanner, TSLexer *lexer, enum TokenType token_type, enum TokenType ending_delimiter, bool can_escape, bool can_interpolate) {
  if (lexer->lookahead == get_end_delimiter(scanner)) {
    lexer->result_symbol = ending_delimiter;
    advance(lexer);
    lexer->mark_end(lexer);
    return true;
  } else {
    // oh boy! the interpolation
    if (can_interpolate && lexer->lookahead == '$') {
      return handle_interpolation(scanner, lexer, token_type);
    }
    // escape sequences, only basic support as of now
    if (can_escape && lexer->lookahead == '\\') {
      return handle_escape_sequence(lexer, token_type);
    }

    if (!lexer->lookahead) {
      lexer->mark_end(lexer);
      return false;
    }

    // handling nested delimiters qq { hello { from { the}}};
    if (lexer->lookahead == scanner->start_delimiter_char) {
      lexer->result_symbol = token_type;
      advance(lexer);
      return scan_nested_delimiters(scanner, lexer, token_type);
    }

    lexer->result_symbol = token_type;
    advance(lexer);
    lexer->mark_end(lexer);
    return true;
  }

  // shouldn't reach here
  return false;
}

static void set_end_delimiter(Scanner *scanner, int32_t start_delimiter) {
  // round, angle, square, curly
  scanner->is_delimiter_enclosing = true;
  if (start_delimiter == '(') {
    scanner->end_delimiter_char = ')';
  } else if (start_delimiter == '<') {
    scanner->end_delimiter_char = '>';
  } else if (start_delimiter == '[') {
    scanner->end_delimiter_char = ']';
  } else if (start_delimiter == '{') {
    scanner->end_delimiter_char = '}';
  } else {
    scanner->is_delimiter_enclosing = false;
    scanner->end_delimiter_char = start_delimiter;
  }
}

static bool process_separator_delimiter(Scanner *scanner, TSLexer *lexer, enum TokenType separator_token, enum TokenType end_token) {
  if (scanner->is_separator_delimiter_parsed) {
    lexer->result_symbol = end_token;
    advance(lexer);
    lexer->mark_end(lexer);
    return true;
  } else {
    lexer->result_symbol = separator_token;
    advance(lexer);
    lexer->mark_end(lexer);

    // if delimiter is {}, (), <>, []
    if (scanner->is_delimiter_enclosing) {
      run_over_spaces(lexer);

      if (lexer->lookahead == scanner->start_delimiter_char) {
        lexer->result_symbol = separator_token;
        advance(lexer);
        lexer->mark_end(lexer);

        scanner->is_separator_delimiter_parsed = true;

        return true;
      }

      return false;
    } else {
      scanner->is_separator_delimiter_parsed = true;

      return true;
    }

    return false;
  }
}

// Give a token type, parses the start delimiter,
// and keeps track of it in memory.
static bool parse_start_delimiter(Scanner *scanner, TSLexer *lexer, enum TokenType token_type) {
  run_over_spaces(lexer);

  scanner->start_delimiter_char = lexer->lookahead;
  set_end_delimiter(scanner, scanner->start_delimiter_char);

  // for substitute and tr/y usecase
  scanner->is_separator_delimiter_parsed = false;

  lexer->result_symbol = token_type;
  advance(lexer);
  lexer->mark_end(lexer);

  return true;
}

/**
 * Consume a "word" in POSIX parlance, and returns it unquoted.
 *
 * This is an approximate implementation that doesn't deal with any
 * POSIX-mandated substitution, and assumes the default value for
 * IFS.
 */
static bool advance_word(Scanner *scanner, TSLexer *lexer) {
  bool empty = true;
  bool has_space_before = false;
  bool allows_interpolation = true;
  char unquoted_word[1000] = "";

  // <<~EOF
  if (lexer->lookahead == '~') {
    Data true_data;
    true_data.boolean = true;
    enqueue(scanner->heredoc.heredoc_allows_indent, true_data, BOOLEAN);
    advance(lexer);
  } else {
    Data false_data;
    false_data.boolean = false;
    enqueue(scanner->heredoc.heredoc_allows_indent, false_data, BOOLEAN);
  }

  // <<\EOF, <<~\EOF
  if (lexer->lookahead == '\\') {
    allows_interpolation = false;
    advance(lexer);
  }

  // run over the spaces
  if (iswspace(lexer->lookahead)) {
    run_over_spaces(lexer);
    has_space_before = true;
  }

  int32_t quote = 0;
  if (
    lexer->lookahead == '\''
    || lexer->lookahead == '"'
    || lexer->lookahead == '`'
  ) {
    allows_interpolation = (lexer->lookahead == '\'') ? false : true;
    quote = lexer->lookahead;
    advance(lexer);
  }
  else if (has_space_before) {
    return false;
  }

  while (
    lexer->lookahead
    && is_identifier(lexer)
    && !(quote ? lexer->lookahead == quote : iswspace(lexer->lookahead))
  ) {
    // TODO: check this below condition
    if (lexer->lookahead == '\\') {
      advance(lexer);
      if (!lexer->lookahead)
        return false;
    }
    empty = false;

    // should convert to string?
    strncat(unquoted_word, (char*) &lexer->lookahead, 1);
    advance(lexer);
  }

  if (quote && lexer->lookahead == quote) {
    advance(lexer);
  }

  if (!empty) {
    Data identifier_string_data;
    strncpy(identifier_string_data.string, unquoted_word, 1000);
    enqueue(scanner->heredoc.heredoc_identifier_queue, identifier_string_data, STRING);

    Data allows_interpolation_data;
    allows_interpolation_data.boolean = allows_interpolation;
    enqueue(scanner->heredoc.heredoc_allows_interpolation, allows_interpolation_data, BOOLEAN);

    scanner->heredoc.started_heredoc = true;
  }

  // STRING_FREE(unquoted_word);

  return !empty;
}

static bool exit_if_heredoc_end_delimiter(Scanner *scanner, TSLexer *lexer) {
  char word[1000] = "";
  lexer->result_symbol = HEREDOC_END_IDENTIFIER;
  while (!iswspace(lexer->lookahead)) {
    // printf("string here - %c", lexer->lookahead);
    // STRING_PUSH(word, lexer->lookahead);
    strncat(word, (char*) &lexer->lookahead, 1);
    advance(lexer);

    if (!lexer->lookahead) {
      break;
    }
  }

  // printf ("found word inside exit %s \n", word.data);
  // printf ("front of identifier queue %s \n", front(scanner->heredoc.heredoc_identifier_queue));

  // if (word == front(scanner->heredoc.heredoc_identifier_queue).data)
  if (! strcmp(word, peek(scanner->heredoc.heredoc_identifier_queue).string)) {
    lexer->result_symbol = HEREDOC_END_IDENTIFIER;
    lexer->mark_end(lexer);

    // unset stuffs
    scanner->heredoc.started_heredoc = false;
    scanner->heredoc.started_heredoc_body = false;
    dequeue(scanner->heredoc.heredoc_identifier_queue);
    dequeue(scanner->heredoc.heredoc_allows_interpolation);
    return true;
  }
  else {
    lexer->result_symbol = HEREDOC_CONTENT;
    return true;
  }

  // STRING_FREE(word);
}

static bool isSpecialVariableIdentifier(TSLexer *lexer) {
  if (
    isdigit(lexer->lookahead) // 0-9
    // || lexer->lookahead == 'a' // ab
    // || lexer->lookahead == 'b'
    || lexer->lookahead == '!'
    || lexer->lookahead == '_'
    || lexer->lookahead == '"'
    // || lexer->lookahead == '#' // no longer supported as part of perl 5.30
    || lexer->lookahead == '$'
    || lexer->lookahead == '%'
    || lexer->lookahead == '&'
    || lexer->lookahead == '\''
    || lexer->lookahead == '('
    || lexer->lookahead == ')'
    // || lexer->lookahead == '*' // no longer supported as part of perl 5.30
    || lexer->lookahead == '+'
    || lexer->lookahead == '-'
    || lexer->lookahead == '.'
    || lexer->lookahead == '/'
    || lexer->lookahead == ':'
    || lexer->lookahead == ';'
    || lexer->lookahead == '<'
    || lexer->lookahead == '='
    || lexer->lookahead == '>'
    || lexer->lookahead == '?'
    || lexer->lookahead == '@'
    || lexer->lookahead == ']'
    || lexer->lookahead == '['
    || lexer->lookahead == '\\'
    || lexer->lookahead == '`'
    || lexer->lookahead == '|'
    || lexer->lookahead == '~'
  ) {
    return true;
  }
  return false;
}

static inline bool scan(Scanner *scanner, TSLexer *lexer, const bool *valid_symbols) {
  // on ERROR, external scanner is called with all valid_symbols to be true.
  // so for our usecase we need this logic.
  // ref - https://github.com/tree-sitter/tree-sitter/issues/1128
  if (
      valid_symbols[SCALAR_VARIABLE_EXTERNAL]
      && valid_symbols[START_DELIMITER]
      && valid_symbols[END_DELIMITER]
      && valid_symbols[STRING_CONTENT]
      && valid_symbols[STRING_SINGLE_QUOTED_CONTENT]
      && valid_symbols[STRING_SINGLE_Q_QUOTED_CONTENT]
      && valid_symbols[STRING_QQ_QUOTED_CONTENT]
      && valid_symbols[STRING_DOUBLE_QUOTED_CONTENT] && valid_symbols[START_DELIMITER_QW] && valid_symbols[END_DELIMITER_QW] && valid_symbols[START_DELIMITER_REGEX] && valid_symbols[REGEX_PATTERN] && valid_symbols[END_DELIMITER_REGEX] && valid_symbols[START_DELIMITER_SEARCH_REPLACE] && valid_symbols[SEARCH_REPLACE_CONTENT] && valid_symbols[SEPARATOR_DELIMITER_SEARCH_REPLACE] && valid_symbols[END_DELIMITER_SEARCH_REPLACE] && valid_symbols[START_DELIMITER_TRANSLITERATION] && valid_symbols[TRANSLITERATION_CONTENT] && valid_symbols[SEPARATOR_DELIMITER_TRANSLITERATION] && valid_symbols[END_DELIMITER_TRANSLITERATION] && valid_symbols[IMAGINARY_HEREDOC_START] && valid_symbols[HEREDOC_START_IDENTIFIER] && valid_symbols[HEREDOC_CONTENT] && valid_symbols[HEREDOC_END_IDENTIFIER] && valid_symbols[POD_CONTENT]
      && valid_symbols[DATA_NOT_FOR_COMPILER]
      && valid_symbols[START_DELIMITER_SLASH]
      && valid_symbols[AUTOMATIC_SEMICOLON]) {
    return false;
  }

  if (valid_symbols[SCALAR_VARIABLE_EXTERNAL]) {
    // run over spaces
    run_over_spaces(lexer);

    // some exit conditions
    if (!lexer->lookahead) {
      lexer->mark_end(lexer);
      return false;
    }
    
    bool met_identifier = false;
    // $_abc is not valid
    // ie, any scalar variable starting with special scalar identifier is not valid
    if (isSpecialVariableIdentifier(lexer)) {

      // special case for 0-9 and ab
      // NOTE/TODO: removed ab as of now
      // uncomment below two lines to make it work?
      if (
        // lexer->lookahead == 'a'
        // || lexer->lookahead == 'b'
        isdigit(lexer->lookahead)
      ) {
        advance(lexer);
        if (iswalnum(lexer->lookahead)) {
          lexer->result_symbol = SCALAR_VARIABLE_EXTERNAL;
          advance(lexer);
          met_identifier = true;
        }
        else {
          lexer->mark_end(lexer);
          return false;
        }
      }
      else {
        lexer->mark_end(lexer);
        return false;
      }
    }
    // # -> length of array. ex $#array
    else if (lexer->lookahead == '#' || lexer->lookahead == '*') {
      lexer->result_symbol = SCALAR_VARIABLE_EXTERNAL;
      advance(lexer);
    }
    // $^A
    else if (lexer->lookahead == '^') {
      lexer->result_symbol = SCALAR_VARIABLE_EXTERNAL;
      advance(lexer);
      if (iswupper(lexer->lookahead) && iswalpha(lexer->lookahead)) {
        advance(lexer);
        lexer->mark_end(lexer);
        return true;
      }
      else {
        lexer->mark_end(lexer);
        return false;
      }
    }

    while (is_identifier(lexer)) {
      lexer->result_symbol = SCALAR_VARIABLE_EXTERNAL;
      advance(lexer);
      met_identifier = true;
    }

    lexer->mark_end(lexer);
    return met_identifier;
  }

  if (valid_symbols[STRING_SINGLE_QUOTED_CONTENT]) {
    // end when you reach the final single quote '
    if (lexer->lookahead == '\'') {
      lexer->mark_end(lexer);
      advance(lexer);
      return false;
    }
    // check for escaped single quote \'
    else if (lexer->lookahead == '\\') {
      lexer->result_symbol = STRING_SINGLE_QUOTED_CONTENT;
      advance(lexer);

      if (lexer->lookahead == '\'') {
        advance(lexer);
      }
      lexer->mark_end(lexer);
      return true;
    }

    // some exit conditions
    if (!lexer->lookahead) {
      lexer->mark_end(lexer);
      return false;
    }

    lexer->result_symbol = STRING_SINGLE_QUOTED_CONTENT;
    advance(lexer);
    lexer->mark_end(lexer);

    return true;
  }

  // TODO: handle qqqSTRINGq; - this should throw error
  if (valid_symbols[START_DELIMITER]) {
    return parse_start_delimiter(scanner, lexer, START_DELIMITER);
  }

  if (valid_symbols[STRING_SINGLE_Q_QUOTED_CONTENT]) {
    return parse_delimited_and_interpolated_content(scanner, lexer, STRING_SINGLE_Q_QUOTED_CONTENT, END_DELIMITER, 0, 0);
  }

  if (valid_symbols[STRING_QQ_QUOTED_CONTENT]) {
    return parse_delimited_and_interpolated_content(scanner, lexer, STRING_QQ_QUOTED_CONTENT, END_DELIMITER, 1, 1);
  }

  if (valid_symbols[STRING_DOUBLE_QUOTED_CONTENT]) {
    if (lexer->lookahead == '"') {
      lexer->mark_end(lexer);
      advance(lexer);
      return false;
    }

    // oh boy! the interpolation
    if (lexer->lookahead == '$') {
      return handle_interpolation(scanner, lexer, STRING_DOUBLE_QUOTED_CONTENT);
    }
    // escape sequences, only basic support as of now
    if (lexer->lookahead == '\\') {
      return handle_escape_sequence(lexer, STRING_DOUBLE_QUOTED_CONTENT);
    }

    // some exit conditions
    if (!lexer->lookahead) {
      lexer->mark_end(lexer);
      return false;
    }

    lexer->result_symbol = STRING_DOUBLE_QUOTED_CONTENT;
    advance(lexer);
    lexer->mark_end(lexer);
    return true;
  }

  if (valid_symbols[START_DELIMITER_QW]) {
    return parse_start_delimiter(scanner, lexer, START_DELIMITER_QW);
  }

  if (valid_symbols[ELEMENT_IN_QW]) {
    run_over_spaces(lexer);

    if (lexer->lookahead == get_end_delimiter(scanner)) {
      lexer->result_symbol = END_DELIMITER_QW;
      advance(lexer);
      lexer->mark_end(lexer);
      return true;
    }

    // exit condition
    if (!lexer->lookahead) {
      lexer->mark_end(lexer);
      return false;
    }

    while (
        lexer->lookahead  // exit condition
        && lexer->lookahead != ' ' && lexer->lookahead != '\t' && lexer->lookahead != '\r' && lexer->lookahead != '\n' && lexer->lookahead != get_end_delimiter(scanner)) {
      lexer->result_symbol = ELEMENT_IN_QW;
      advance(lexer);
    }

    lexer->mark_end(lexer);
    return true;
  }

  // for // regex patterns
  if (valid_symbols[START_DELIMITER_SLASH]) {
    if (lexer->lookahead == '/') {
      return parse_start_delimiter(scanner, lexer, START_DELIMITER_SLASH);
    }
    else {
      lexer->mark_end(lexer);
      return false;
    }
  }

  // if (valid_symbols[START_DELIMITER_SLASH]) {
  //   return parse_start_delimiter(scan, lexer, START_DELIMITER_SLASH);
  // }

  if (valid_symbols[START_DELIMITER_REGEX]) {
    return parse_start_delimiter(scanner, lexer, START_DELIMITER_REGEX);
  }
  if (valid_symbols[REGEX_PATTERN]) {
    return parse_delimited_and_interpolated_content(scanner, lexer, REGEX_PATTERN, END_DELIMITER_REGEX, 1, 1);
  }

  if (valid_symbols[START_DELIMITER_SEARCH_REPLACE]) {
    return parse_start_delimiter(scanner, lexer, START_DELIMITER_SEARCH_REPLACE);
  }

  if (valid_symbols[SEARCH_REPLACE_CONTENT]) {
    if (lexer->lookahead == get_end_delimiter(scanner)) {
      return process_separator_delimiter(scanner, lexer, SEPARATOR_DELIMITER_SEARCH_REPLACE, END_DELIMITER_SEARCH_REPLACE);
    } else {
      // oh boy! the interpolation
      if (lexer->lookahead == '$') {
        return handle_interpolation(scanner, lexer, SEARCH_REPLACE_CONTENT);
      }
      // escape sequences, only basic support as of now
      if (lexer->lookahead == '\\') {
        return handle_escape_sequence(lexer, SEARCH_REPLACE_CONTENT);
      }

      // some exit conditions
      if (!lexer->lookahead) {
        lexer->mark_end(lexer);
        return false;
      }

      // handling nested delimiters qq { hello { from { the}}};
      if (lexer->lookahead == scanner->start_delimiter_char) {
        lexer->result_symbol = SEARCH_REPLACE_CONTENT;
        advance(lexer);
        return scan_nested_delimiters(scanner, lexer, SEARCH_REPLACE_CONTENT);
      }

      lexer->result_symbol = SEARCH_REPLACE_CONTENT;
      advance(lexer);
      return true;
    }
  }

  if (valid_symbols[START_DELIMITER_TRANSLITERATION]) {
    return parse_start_delimiter(scanner, lexer, START_DELIMITER_TRANSLITERATION);
  }
  if (valid_symbols[TRANSLITERATION_CONTENT]) {
    if (lexer->lookahead == get_end_delimiter(scanner)) {
      return process_separator_delimiter(scanner, lexer, SEPARATOR_DELIMITER_TRANSLITERATION, END_DELIMITER_TRANSLITERATION);
    }

    // exit condition
    if (!lexer->lookahead) {
      lexer->mark_end(lexer);
      return false;
    }

    // escape sequence
    if (lexer->lookahead == '\\') {
      lexer->result_symbol = TRANSLITERATION_CONTENT;
      advance(lexer);
      // self end delimiter
      if (lexer->lookahead == get_end_delimiter(scanner)) {
        advance(lexer);
      }

      lexer->mark_end(lexer);
      return true;
    }

    // handling nested delimiters qq { hello { from { the}}};
    if (lexer->lookahead == scanner->start_delimiter_char) {
      lexer->result_symbol = TRANSLITERATION_CONTENT;
      advance(lexer);
      return scan_nested_delimiters(scanner, lexer, TRANSLITERATION_CONTENT);
    }

    lexer->result_symbol = TRANSLITERATION_CONTENT;
    advance(lexer);
    lexer->mark_end(lexer);
    return true;
  }

  if (valid_symbols[HEREDOC_START_IDENTIFIER]) {
    lexer->result_symbol = HEREDOC_START_IDENTIFIER;

    // printf("before...\n");
    bool found_delimiter = advance_word(scanner, lexer);
    // printf("found delimieter %d \n", found_delimiter);
    // printf("found identifier %d \n", scanner->heredoc.heredoc_identifier_queue->size);
    return found_delimiter;
  }

  if (
      (valid_symbols[HEREDOC_CONTENT] || valid_symbols[IMAGINARY_HEREDOC_START])
      && !is_empty(scanner->heredoc.heredoc_identifier_queue)
  ) {
    // another exit condition
    if (!lexer->lookahead && !scanner->heredoc.started_heredoc_body) {
      return false;
    }

    if (lexer->lookahead == '\n' && !scanner->heredoc.started_heredoc_body) {
      scanner->heredoc.started_heredoc_body = true;

      lexer->result_symbol = IMAGINARY_HEREDOC_START;
      lexer->mark_end(lexer);
      return true;
    }
    // printf("started heredoc body %d \n", scanner->heredoc.started_heredoc_body);
    if (scanner->heredoc.started_heredoc_body) {
      switch (lexer->lookahead) {
        case '\\': {
          if (peek(scanner->heredoc.heredoc_allows_interpolation).boolean) {
            return handle_escape_sequence(lexer, HEREDOC_CONTENT);
          }
        }

        case '$': {
          if (peek(scanner->heredoc.heredoc_allows_interpolation).boolean) {
            return false;
          }
        }

        case '\n': {
          skip(lexer);
          lexer->mark_end(lexer);
          // TODO: validate all possible intended heredocs properly
          if (peek(scanner->heredoc.heredoc_allows_indent).boolean) {
            while (iswspace(lexer->lookahead)) {
              advance(lexer);
            }
          }
          return exit_if_heredoc_end_delimiter(scanner, lexer);
        }

        default: {
          // exit condition
          if (!lexer->lookahead) {
            scanner->heredoc.started_heredoc_body = false;
            lexer->mark_end(lexer);
            return false;
          }
          lexer->result_symbol = HEREDOC_CONTENT;
          advance(lexer);
          return true;
        }
      }
    } else {
      return false;
    }
  }

  if (valid_symbols[POD_CONTENT]) {
    while (lexer->lookahead) {
      lexer->result_symbol = POD_CONTENT;

      // if it is =cut that marks the end of pod content
      // it should be newline with no spaces before
      if (lexer->lookahead == '\n') {
        lexer->advance(lexer, false);
        if (lexer->lookahead == '=') {
          lexer->advance(lexer, false);
          if (lexer->lookahead == 'c') {
            lexer->advance(lexer, false);
            if (lexer->lookahead == 'u') {
              lexer->advance(lexer, false);
              if (lexer->lookahead == 't') {
                lexer->advance(lexer, false);
                while(lexer->lookahead && lexer->lookahead != '\n') {
                  lexer->advance(lexer, false);
                }
                lexer->mark_end(lexer);
                return true;
              }
            }
          }
        }
      }
      else {
        lexer->advance(lexer, false);
      }
    }

    // or if it end of the file also, mark the end of pod content
    lexer->mark_end(lexer);
    return true;
  }

  if (valid_symbols[DATA_NOT_FOR_COMPILER]) {
    lexer->result_symbol = DATA_NOT_FOR_COMPILER;
    while(lexer->lookahead) {
      advance(lexer);
    }

    lexer->mark_end(lexer);
    return true;
  }
  if (valid_symbols[AUTOMATIC_SEMICOLON]) {
    for (;;) {
      if (lexer->lookahead == '}'|| !lexer->lookahead) {
        lexer->result_symbol = AUTOMATIC_SEMICOLON;
        lexer->mark_end(lexer);
        return true;
      }
      else if (lexer->lookahead == ';' || !iswspace(lexer->lookahead)) {
        return false;
      }
      else if (iswspace(lexer->lookahead)) {
        skip(lexer);
      }
    }
    

    return false;
  }

  return false;
}

static unsigned serialize(Scanner *scanner, char *buffer) {
  uint32_t size = 0;

  return size;
}

// static inline void reset_heredoc(StringQueue *queue) {
//   queue->data = NULL;
// }

static void reset (Scanner *scanner) {
  scanner->heredoc.started_heredoc = false;
  scanner->heredoc.started_heredoc_body = false;
  // for (int i = 0; i < scanner->heredoc.heredoc_identifier_queue->size; i++) {
    // reset_heredoc(&scanner->heredoc.heredoc_identifier_queue[i]);
  // }
}

static void deserialize(Scanner *scanner, const char *buffer, unsigned length) {
  // printf("sdf %d \n", length);
  // if (length == 0) {
  //   reset(scanner);
  // }
}

void *tree_sitter_perl_external_scanner_create() {
  Scanner *scanner = malloc(sizeof(Scanner));
  scanner->heredoc = heredoc_new();
  return scanner;
}

unsigned tree_sitter_perl_external_scanner_serialize(
    void *payload,
    char *buffer) {
  Scanner *scanner = (Scanner *)payload;
  return serialize(scanner, buffer);
}

void tree_sitter_perl_external_scanner_deserialize(
    void *payload,
    const char *buffer,
    unsigned length) {
  Scanner *scanner = (Scanner *)payload;
  deserialize(scanner, buffer, length);  // TODO: need to deserialize heredoc
}

bool tree_sitter_perl_external_scanner_scan(
    void *payload,
    TSLexer *lexer,
    const bool *valid_symbols) {
  Scanner *scanner = (Scanner *)payload;
  return scan(scanner, lexer, valid_symbols);
}

void tree_sitter_perl_external_scanner_destroy(void *payload) {
  Scanner *scanner = (Scanner *)payload;
  // deleteBoolQueue(scanner->heredoc.heredoc_allows_indent);
  // deleteBoolQueue(scanner->heredoc.heredoc_allows_interpolation);
  // deleteStringQueue(scanner->heredoc.heredoc_identifier_queue);
  // for (size_t i = 0; i < scanner->heredocs.len; i++) {
  //     Heredoc *heredoc = &scanner->heredocs.data[i];
  //     STRING_FREE(heredoc->current_leading_word);
  //     STRING_FREE(heredoc->delimiter);
  // }
  // VEC_FREE(scanner->heredocs);
  free(scanner);
}
