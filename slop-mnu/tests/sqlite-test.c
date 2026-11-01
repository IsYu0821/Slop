/* SQLite via manual prototypes (no dev headers on host), linked against
   unmodified host libsqlite3.so.0. Exercises real SQLite on guest ext4. */
#include <stdio.h>
#include <string.h>
typedef struct sqlite3 sqlite3;
typedef struct sqlite3_stmt sqlite3_stmt;
#define SQLITE_OK 0
#define SQLITE_ROW 100
extern int sqlite3_open(const char*,sqlite3**);
extern int sqlite3_exec(sqlite3*,const char*,int(*)(void*,int,char**,char**),void*,char**);
extern int sqlite3_close(sqlite3*);
extern const char *sqlite3_errmsg(sqlite3*);
extern int sqlite3_prepare_v2(sqlite3*,const char*,int,sqlite3_stmt**,const char**);
extern int sqlite3_step(sqlite3_stmt*);
extern int sqlite3_column_int(sqlite3_stmt*,int);
extern int sqlite3_finalize(sqlite3_stmt*);
int main(void){
 sqlite3 *db=0;
 if(sqlite3_open("/tmp/sqlite-test.db",&db)!=SQLITE_OK){printf("SQLITE_FAIL open %s\n",sqlite3_errmsg(db));return 1;}
 char *err=0;
 if(sqlite3_exec(db,"CREATE TABLE t(id INTEGER PRIMARY KEY, v TEXT);",0,0,&err)!=SQLITE_OK){printf("SQLITE_FAIL create %s\n",err);return 1;}
 if(sqlite3_exec(db,"BEGIN;",0,0,&err)!=SQLITE_OK){puts("SQLITE_FAIL begin");return 1;}
 for(int i=0;i<100;i++){
  char sql[64]; snprintf(sql,sizeof sql,"INSERT INTO t VALUES(%d,'val%d');",i,i);
  if(sqlite3_exec(db,sql,0,0,&err)!=SQLITE_OK){printf("SQLITE_FAIL insert %d %s\n",i,err);return 1;}
 }
 if(sqlite3_exec(db,"COMMIT;",0,0,&err)!=SQLITE_OK){puts("SQLITE_FAIL commit");return 1;}
 if(sqlite3_exec(db,"UPDATE t SET v='upd' WHERE id=42;",0,0,&err)!=SQLITE_OK){puts("SQLITE_FAIL update");return 1;}
 if(sqlite3_exec(db,"DELETE FROM t WHERE id<10;",0,0,&err)!=SQLITE_OK){puts("SQLITE_FAIL delete");return 1;}
 sqlite3_close(db);
 if(sqlite3_open("/tmp/sqlite-test.db",&db)!=SQLITE_OK){puts("SQLITE_FAIL reopen");return 1;}
 sqlite3_stmt *st=0;
 if(sqlite3_prepare_v2(db,"SELECT count(*), sum(id) FROM t;",-1,&st,0)!=SQLITE_OK){puts("SQLITE_FAIL select");return 1;}
 int rc=sqlite3_step(st);
 if(rc!=SQLITE_ROW){printf("SQLITE_FAIL step %d\n",rc);return 1;}
 int cnt=sqlite3_column_int(st,0);
 int sum=sqlite3_column_int(st,1);
 sqlite3_finalize(st);
 sqlite3_close(db);
 if(cnt!=90||sum!=4905){printf("SQLITE_FAIL count=%d sum=%d\n",cnt,sum);return 1;}
 printf("SQLITE_OK count=%d sum=%d\n",cnt,sum);
 return 0;
}
