#ifndef BACKUPDUCK_H
#define BACKUPDUCK_H
#ifdef __cplusplus
extern "C" {
#endif
// Blocking, thread-safe calls. Do not log JSON: pairing contains credentials.
char *backupduck_call(const char *request);
void backupduck_free(char *response);
#ifdef __cplusplus
}
#endif
#endif
