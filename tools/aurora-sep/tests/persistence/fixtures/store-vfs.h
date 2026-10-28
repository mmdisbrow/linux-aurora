/* SPDX-License-Identifier: GPL-2.0-only OR MIT */
/* In-memory VFS boundary with inode/descriptor identity and durable namespace. */
#define _GNU_SOURCE
#include <assert.h>
#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/types.h>
#ifndef O_LARGEFILE
#define O_LARGEFILE 0
#endif
#define GFP_KERNEL 0
#define IS_ERR(p) ((uintptr_t)(p) >= (uintptr_t)(-4095))
#define PTR_ERR(p) ((long)(intptr_t)(p))
#define PTR_ERR_OR_ZERO(p) (IS_ERR(p) ? PTR_ERR(p) : 0)
#define ERR_PTR(n) ((void *)(intptr_t)(n))
#define QSTR_INIT(n, l) { .name = (n), .len = (l) }
#define min_t(t, a, b) ((t)(a) < (t)(b) ? (t)(a) : (t)(b))
/* This VFS models regular files only; block-device leaves below abort. */
#define S_ISBLK(x) (0)
#define CAP (72U * 32768 + 32)
typedef uint64_t u64;
/* Supply the existing kernel ABI name to the host-compiled production code. */
typedef unsigned int umode_t;
struct file;
struct inode {
	unsigned int i_mode;
	struct file *file;
};
struct dentry {
	struct inode *inode;
	struct dentry *parent;
	bool linked;
};
struct path {
	struct dentry *dentry;
	void *mnt;
};
struct file {
	struct path f_path;
	struct dentry entry;
	struct inode inode;
	unsigned char *cache, *disk;
	size_t len, disk_len;
	int refs, id;
	bool active;
};
struct cred {
	int unused;
};
struct qstr {
	const char *name;
	size_t len;
};
struct renamedata {
	void *mnt_idmap;
	struct dentry *old_parent, *old_dentry, *new_parent, *new_dentry;
	void *delegated_inode;
	unsigned int flags;
};
static struct {
	void *fs;
} init_task;
static struct cred cred;
static struct file directory;
static struct dentry root_entry, negative_entry;
static struct file *files[32], *target, *durable_target, *pending;
static int next_id, mount_token, idmap_token, mounts, locks, credentials, roots,
	allocations;
static int publication, writes, syncs, renames, dirsyncs, old_unlinked_writes,
	source_reads;
static int fault_stage, fault_publication, fault_error, fail_outside_sync,
	fail_outside_write;
static size_t fault_at, read_limit;
static bool armed;
enum {
	HEADER_WRITE = 1,
	TAIL_WRITE,
	TAIL_READ_ERROR,
	TAIL_READ_EOF,
	BUFFER_ALLOC,
	TEMP_FSYNC,
	RENAME_FAIL,
	DIR_FSYNC,
	TARGET_CHANGED,
	TARGET_MISSING,
	SOURCE_SHRINK,
	SOURCE_GROW,
	SOURCE_SHRINK_DURING,
	SOURCE_GROW_DURING
};
static struct inode *d_inode(struct dentry *d)
{
	return d->inode;
}
static struct inode *file_inode(struct file *f)
{
	return &f->inode;
}
static loff_t i_size_read(struct inode *i)
{
	return i->file->len;
}
static void *file_bdev(struct file *f)
{
	(void)f;
	abort();
}
static long long bdev_nr_bytes(void *p)
{
	(void)p;
	abort();
}
static struct file *new_file(void)
{
	int i;

	for (i = 0; i < 32 && files[i]; i++)
		;
	assert(i < 32);
	struct file *f = calloc(1, sizeof(*f));

	assert(f);
	f->cache = calloc(1, CAP);
	f->disk = calloc(1, CAP);
	assert(f->cache && f->disk);
	f->id = ++next_id;
	f->active = true;
	f->inode = (struct inode){ 1, f };
	f->entry = (struct dentry){ &f->inode, directory.f_path.dentry, true };
	f->f_path = (struct path){ &f->entry, &mount_token };
	files[i] = f;
	return f;
}
static void collect(void)
{
	for (int i = 0; i < 32; i++) {
		struct file *f = files[i];

		if (f && !f->refs && !f->entry.linked && f != durable_target) {
			if (pending == f)
				pending = NULL;
			free(f->cache);
			free(f->disk);
			free(f);
			files[i] = NULL;
		}
	}
}
void fixture_reset(void)
{
	memset(files, 0, sizeof(files));
	memset(&directory, 0, sizeof(directory));
	directory.inode = (struct inode){ 2, &directory };
	directory.entry =
		(struct dentry){ &directory.inode, &root_entry, true };
	directory.f_path = (struct path){ &directory.entry, &mount_token };
	memset(&negative_entry, 0, sizeof(negative_entry));
	target = durable_target = pending = NULL;
	next_id = mounts = locks = credentials = roots = allocations = 0;
	publication = writes = syncs = renames = dirsyncs =
		old_unlinked_writes = source_reads = 0;
	fault_stage = fault_publication = fault_error = fail_outside_sync =
		fail_outside_write = 0;
	fault_at = read_limit = 0;
	armed = false;
}
void fixture_done(void)
{
	assert(!mounts && !locks && !credentials && !roots && !allocations &&
	       !directory.refs);
	for (int i = 0; i < 32; i++)
		if (files[i]) {
			assert(files[i]->refs == 0);
			free(files[i]->cache);
			free(files[i]->disk);
			free(files[i]);
			files[i] = NULL;
		}
}
void fixture_arm(int stage, int which, size_t at, int error)
{
	fault_stage = stage;
	fault_publication = which;
	fault_at = at;
	fault_error = error;
	publication = writes = syncs = renames = dirsyncs = source_reads = 0;
	armed = true;
}
void fixture_clear(void)
{
	fault_stage = fail_outside_sync = fail_outside_write = 0;
	armed = false;
	read_limit = 0;
}
void fixture_sync_error(int nth)
{
	fail_outside_sync = nth;
	syncs = 0;
}
void fixture_data_error(int nth)
{
	fail_outside_write = nth;
	writes = 0;
}
void fixture_short_reads(size_t n)
{
	read_limit = n;
}
int fixture_count(int n)
{
	switch (n) {
	case 0:
		return renames;
	case 1:
		return publication;
	case 2:
		return old_unlinked_writes;
	case 3:
		return dirsyncs;
	case 4:
		return target ? target->id : 0;
	case 5:
		return source_reads;
	default:
		abort();
	}
}
void fixture_crash(void)
{
	for (int i = 0; i < 32; i++)
		if (files[i])
			assert(files[i]->refs == 0);
	target = durable_target;
	for (int i = 0; i < 32; i++)
		if (files[i]) {
			struct file *f = files[i];

			f->entry.linked = f == target;
			memcpy(f->cache, f->disk, CAP);
			f->len = f->disk_len;
		}
	collect();
	fixture_clear();
}
static bool fault(int stage)
{
	return armed && fault_stage == stage &&
	       publication == fault_publication;
}
static void *kmalloc(size_t n, int flags)
{
	assert(flags == GFP_KERNEL && n == 32768);
	if (fault(BUFFER_ALLOC))
		return NULL;
	void *p = malloc(n);

	assert(p);
	allocations++;
	return p;
}
static char *kstrndup(const char *p, size_t n, int flags)
{
	assert(flags == GFP_KERNEL);
	char *s = malloc(n + 1);

	assert(s);
	memcpy(s, p, n);
	s[n] = 0;
	allocations++;
	return s;
}
static void kfree(void *p)
{
	free(p);
	allocations--;
	assert(allocations >= 0);
}
static struct cred *prepare_kernel_cred(void *p)
{
	assert(p == &init_task);
	return &cred;
}
static void task_lock(void *p)
{
	assert(p == &init_task);
}
static void task_unlock(void *p)
{
	assert(p == &init_task);
}
static void get_fs_root(void *fs, struct path *p)
{
	(void)fs;
	*p = (struct path){ &root_entry, &mount_token };
	roots++;
}
static void path_put(struct path *p)
{
	assert(p->dentry == &root_entry);
	roots--;
}
static const struct cred *override_creds(struct cred *p)
{
	assert(p == &cred);
	credentials++;
	return &cred;
}
static struct cred *revert_creds(const struct cred *p)
{
	assert(p == &cred);
	credentials--;
	return &cred;
}
static void put_cred(struct cred *p)
{
	assert(p == &cred);
}
static struct file *open_as_kernel(const char *p, int flags, umode_t mode)
{
	assert(!strcmp(p, "/var/lib/aurora-sep-host-state.bin"));
	if ((flags & O_EXCL) && target)
		return ERR_PTR(-EEXIST);
	if (!target) {
		if (!(flags & O_CREAT))
			return ERR_PTR(-ENOENT);
		assert(mode == 0600);
		target = new_file();
	}
	target->refs++;
	return target;
}
static struct file *file_open_root(const struct path *p, const char *name,
				   int flags, umode_t mode)
{
	assert(credentials == 1);
	if (p->dentry == &root_entry) {
		assert(!strcmp(name, "/var/lib"));
		assert(flags == (O_RDONLY | O_DIRECTORY | O_LARGEFILE) &&
		       mode == 0);
		directory.refs++;
		return &directory;
	}
	assert(p == &directory.f_path);
	assert((flags & (O_CREAT | O_EXCL | O_NOFOLLOW)) ==
		       (O_CREAT | O_EXCL | O_NOFOLLOW) &&
	       mode == 0600);
	assert(!strncmp(name, ".sep-", 5));
	publication++;
	pending = new_file();
	pending->refs = 1;
	if (fault(SOURCE_SHRINK)) {
		assert(target);
		target->len = fault_at;
	}
	if (fault(SOURCE_GROW)) {
		assert(target && target->len + 1 <= CAP);
		target->len++;
	}
	return pending;
}
static void get_random_bytes(void *p, size_t n)
{
	memset(p, next_id + 1, n);
}
static ssize_t kernel_read(struct file *f, void *buf, size_t n, loff_t *pos)
{
	assert(f && f->active && f->refs > 0 && *pos >= 0);
	if (credentials) {
		source_reads++;
		if ((size_t)*pos >= fault_at && fault(SOURCE_SHRINK_DURING)) {
			f->len = *pos;
			fault_stage = 0;
		}
		if ((size_t)*pos >= fault_at && fault(SOURCE_GROW_DURING)) {
			f->len++;
			fault_stage = 0;
		}
		if ((size_t)*pos >= fault_at) {
			if (fault(TAIL_READ_ERROR))
				return -fault_error;
			if (fault(TAIL_READ_EOF))
				return 0;
		}
	}
	if ((size_t)*pos >= f->len)
		return 0;
	if (n > f->len - (size_t)*pos)
		n = f->len - (size_t)*pos;
	if (credentials && read_limit && n > read_limit)
		n = read_limit;
	memcpy(buf, f->cache + *pos, n);
	*pos += n;
	return n;
}
static ssize_t kernel_write(struct file *f, const void *buf, size_t n,
			    loff_t *pos)
{
	assert(f && f->active && f->refs > 0 && *pos >= 0 &&
	       (size_t)*pos + n <= CAP && !mounts);
	if (f != pending && f != target)
		old_unlinked_writes++;
	if (!credentials) {
		writes++;
		if (fail_outside_write && writes == fail_outside_write)
			return -EIO;
	}
	bool cut = (credentials && ((fault(HEADER_WRITE) && *pos < 32768) ||
				    (fault(TAIL_WRITE) && *pos >= 32768)));
	if (cut && (size_t)*pos >= fault_at)
		return -fault_error;
	if (cut && n > fault_at - (size_t)*pos)
		n = fault_at - (size_t)*pos;
	memcpy(f->cache + *pos, buf, n);
	*pos += n;
	if ((size_t)*pos > f->len)
		f->len = *pos;
	return n;
}
static int vfs_fsync(struct file *f, int data)
{
	assert(!data && !mounts && !locks);
	if (f == &directory) {
		dirsyncs++;
		if (fault(DIR_FSYNC)) {
			if (fault_error < 0) {
				durable_target = target;
				collect();
				return fault_error;
			}
			return -fault_error;
		}
		durable_target = target;
		collect();
		return 0;
	}
	assert(f && f->refs > 0);
	if (credentials && fault(TEMP_FSYNC))
		return -fault_error;
	if (!credentials) {
		syncs++;
		if (fail_outside_sync && syncs == fail_outside_sync)
			return -EIO;
	}
	memcpy(f->disk, f->cache, CAP);
	f->disk_len = f->len;
	return 0;
}
static int mnt_want_write(void *p)
{
	assert(p == &mount_token && !mounts);
	mounts++;
	return 0;
}
static void mnt_drop_write(void *p)
{
	assert(p == &mount_token && mounts == 1 && !locks);
	mounts--;
}
static void *file_mnt_idmap(struct file *f)
{
	assert(f == &directory);
	return &idmap_token;
}
static int start_renaming_dentry(struct renamedata *rd, int flags,
				 struct dentry *old, struct qstr *name)
{
	assert(credentials == 1 && mounts == 1 && !locks && !flags &&
	       rd->mnt_idmap == &idmap_token);
	assert(rd->old_parent == directory.f_path.dentry &&
	       rd->new_parent == directory.f_path.dentry &&
	       old == pending->f_path.dentry);
	assert(name->len == strlen(name->name) &&
	       !strcmp(name->name, "aurora-sep-host-state.bin"));
	if (fault(TARGET_CHANGED)) {
		target->entry.linked = false;
		target = new_file();
		memcpy(target->cache, "external", 8);
		target->len = 8;
	}
	if (fault(TARGET_MISSING)) {
		target->entry.linked = false;
		target = NULL;
	}
	locks++;
	rd->old_dentry = old;
	rd->new_dentry = target ? &target->entry : &negative_entry;
	return 0;
}
static bool d_is_positive(struct dentry *d)
{
	return d->inode != NULL;
}
static bool d_is_reg(struct dentry *d)
{
	return d->inode && d->inode->i_mode == 1;
}
static int security_path_rename(const struct path *a, struct dentry *b,
				const struct path *c, struct dentry *d,
				unsigned int flags)
{
	assert(a == &directory.f_path && c == a &&
	       b == pending->f_path.dentry &&
	       d == (target ? target->f_path.dentry : &negative_entry) &&
	       !flags && locks);
	return 0;
}
static int vfs_rename(struct renamedata *rd)
{
	assert(locks && mounts && credentials && rd->delegated_inode == NULL &&
	       !rd->flags);
	if (fault(RENAME_FAIL))
		return -fault_error;
	if (target)
		target->entry.linked = false;
	target = pending;
	target->entry.linked = true;
	renames++;
	return 0;
}
static void end_renaming(struct renamedata *rd)
{
	assert(rd->old_parent == directory.f_path.dentry && locks == 1);
	locks--;
}
static struct dentry *start_removing_dentry(struct dentry *parent,
					    struct dentry *child)
{
	assert(parent == directory.f_path.dentry &&
	       child == pending->f_path.dentry && mounts && !locks);
	locks++;
	return child;
}
static int security_path_unlink(const struct path *p, struct dentry *d)
{
	assert(p == &directory.f_path && d == pending->f_path.dentry && locks);
	return 0;
}
static int vfs_unlink(void *idmap, struct inode *inode, struct dentry *d,
		      void *delegated)
{
	assert(idmap == &idmap_token && inode == file_inode(&directory) &&
	       d == pending->f_path.dentry && !delegated && locks && mounts);
	assert(pending != target);
	pending->entry.linked = false;
	return 0;
}
static void end_removing(struct dentry *d)
{
	assert(d == pending->f_path.dentry && locks == 1);
	locks--;
}
static void fput(struct file *f)
{
	assert(f && f->refs > 0);
	f->refs--;
	collect();
}
static int filp_close(struct file *f, void *ignored)
{
	assert(!ignored);
	fput(f);
	return 0;
}
#define pr_err(...) \
	do {        \
	} while (0)
#define pr_warn(...) \
	do {         \
	} while (0)
