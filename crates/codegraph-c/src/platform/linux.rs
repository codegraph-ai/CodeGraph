// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Linux kernel platform module
//!
//! Provides platform-specific configurations for parsing Linux kernel code,
//! including header stubs, attribute lists, and ops struct definitions.

use std::collections::HashMap;

use super::{
    CallbackCategory, DetectionPattern, HeaderStubs, OpsFieldDef, OpsStructDef, PlatformModule,
};

/// Linux kernel platform module
pub struct LinuxPlatform {
    header_stubs: HeaderStubs,
    ops_structs: Vec<OpsStructDef>,
    call_normalizations: HashMap<&'static str, &'static str>,
}

impl LinuxPlatform {
    pub fn new() -> Self {
        let mut platform = Self {
            header_stubs: HeaderStubs::new(),
            ops_structs: Vec::new(),
            call_normalizations: HashMap::new(),
        };
        platform.init_header_stubs();
        platform.init_ops_structs();
        platform.init_call_normalizations();
        platform
    }

    fn init_header_stubs(&mut self) {
        // linux/types.h - Core type definitions
        self.header_stubs.add(
            "linux/types.h",
            r#"
typedef unsigned char u8;
typedef unsigned short u16;
typedef unsigned int u32;
typedef unsigned long long u64;
typedef signed char s8;
typedef signed short s16;
typedef signed int s32;
typedef signed long long s64;
typedef u8 __u8;
typedef u16 __u16;
typedef u32 __u32;
typedef u64 __u64;
typedef s8 __s8;
typedef s16 __s16;
typedef s32 __s32;
typedef s64 __s64;
typedef u16 __le16;
typedef u32 __le32;
typedef u64 __le64;
typedef u16 __be16;
typedef u32 __be32;
typedef u64 __be64;
typedef unsigned long size_t;
typedef long ssize_t;
typedef long long loff_t;
typedef int bool;
typedef unsigned int gfp_t;
typedef unsigned int fmode_t;
typedef unsigned short umode_t;
typedef unsigned int dev_t;
typedef int pid_t;
typedef unsigned int uid_t;
typedef unsigned int gid_t;
typedef long long ktime_t;
typedef unsigned long phys_addr_t;
typedef unsigned long long dma_addr_t;
typedef unsigned long long resource_size_t;
typedef unsigned long uintptr_t;
typedef long intptr_t;
typedef long ptrdiff_t;
"#,
        );

        // linux/kernel.h - Core kernel definitions
        self.header_stubs.add(
            "linux/kernel.h",
            r#"
#define NULL ((void *)0)
#define true 1
#define false 0
#define KERN_EMERG ""
#define KERN_ALERT ""
#define KERN_CRIT ""
#define KERN_ERR ""
#define KERN_WARNING ""
#define KERN_NOTICE ""
#define KERN_INFO ""
#define KERN_DEBUG ""
#define ARRAY_SIZE(arr) (sizeof(arr) / sizeof((arr)[0]))
#define container_of(ptr, type, member) ((type *)((char *)(ptr) - offsetof(type, member)))
#define offsetof(TYPE, MEMBER) ((size_t)&((TYPE *)0)->MEMBER)
extern int printk(const char *fmt, ...);
extern void panic(const char *fmt, ...);
extern void *ERR_PTR(long error);
extern long PTR_ERR(const void *ptr);
extern int IS_ERR(const void *ptr);
"#,
        );

        // linux/init.h - Module init/exit
        self.header_stubs.add(
            "linux/init.h",
            r#"
#define __init
#define __exit
#define __initdata
#define __exitdata
#define __initconst
#define __devinit
#define __devexit
#define module_init(x) int init_module(void) { return x(); }
#define module_exit(x) void cleanup_module(void) { x(); }
#define late_initcall(fn)
#define subsys_initcall(fn)
#define fs_initcall(fn)
#define device_initcall(fn)
#define arch_initcall(fn)
#define core_initcall(fn)
#define postcore_initcall(fn)
"#,
        );

        // linux/module.h - Module infrastructure
        self.header_stubs.add(
            "linux/module.h",
            r#"
#define MODULE_LICENSE(license)
#define MODULE_AUTHOR(author)
#define MODULE_DESCRIPTION(desc)
#define MODULE_VERSION(version)
#define MODULE_ALIAS(alias)
#define MODULE_DEVICE_TABLE(type, name)
#define MODULE_FIRMWARE(fw)
#define MODULE_INFO(tag, info)
#define MODULE_PARM_DESC(parm, desc)
#define EXPORT_SYMBOL(sym)
#define EXPORT_SYMBOL_GPL(sym)
#define EXPORT_SYMBOL_NS(sym, ns)
#define EXPORT_SYMBOL_NS_GPL(sym, ns)
#define THIS_MODULE ((struct module *)0)
struct module;
"#,
        );

        // linux/fs.h - File system operations
        self.header_stubs.add(
            "linux/fs.h",
            r#"
struct file;
struct inode;
struct dentry;
struct super_block;
struct file_operations {
    struct module *owner;
    loff_t (*llseek)(struct file *, loff_t, int);
    ssize_t (*read)(struct file *, char *, size_t, loff_t *);
    ssize_t (*write)(struct file *, const char *, size_t, loff_t *);
    int (*open)(struct inode *, struct file *);
    int (*release)(struct inode *, struct file *);
    long (*unlocked_ioctl)(struct file *, unsigned int, unsigned long);
    int (*mmap)(struct file *, struct vm_area_struct *);
    unsigned int (*poll)(struct file *, struct poll_table_struct *);
    int (*fsync)(struct file *, loff_t, loff_t, int);
    int (*flush)(struct file *, void *);
};
struct inode_operations {
    int (*create)(struct inode *, struct dentry *, umode_t, bool);
    struct dentry * (*lookup)(struct inode *, struct dentry *, unsigned int);
    int (*link)(struct dentry *, struct inode *, struct dentry *);
    int (*unlink)(struct inode *, struct dentry *);
    int (*mkdir)(struct inode *, struct dentry *, umode_t);
    int (*rmdir)(struct inode *, struct dentry *);
};
extern int register_chrdev(unsigned int major, const char *name, const struct file_operations *fops);
extern void unregister_chrdev(unsigned int major, const char *name);
"#,
        );

        // linux/device.h - Device model
        self.header_stubs.add(
            "linux/device.h",
            r#"
struct device;
struct device_driver;
struct class;
struct bus_type;
extern int device_register(struct device *dev);
extern void device_unregister(struct device *dev);
extern struct device *device_create(struct class *cls, struct device *parent, dev_t devt, void *drvdata, const char *fmt, ...);
extern void device_destroy(struct class *cls, dev_t devt);
extern int dev_err(const struct device *dev, const char *fmt, ...);
extern int dev_warn(const struct device *dev, const char *fmt, ...);
extern int dev_info(const struct device *dev, const char *fmt, ...);
extern int dev_dbg(const struct device *dev, const char *fmt, ...);
"#,
        );

        // linux/pci.h - PCI driver infrastructure
        self.header_stubs.add(
            "linux/pci.h",
            r#"
struct pci_dev;
struct pci_device_id {
    u32 vendor, device;
    u32 subvendor, subdevice;
    u32 class, class_mask;
    unsigned long driver_data;
};
struct pci_driver {
    const char *name;
    const struct pci_device_id *id_table;
    int (*probe)(struct pci_dev *dev, const struct pci_device_id *id);
    void (*remove)(struct pci_dev *dev);
    int (*suspend)(struct pci_dev *dev, pm_message_t state);
    int (*resume)(struct pci_dev *dev);
    void (*shutdown)(struct pci_dev *dev);
    struct device_driver driver;
};
extern int pci_register_driver(struct pci_driver *drv);
extern void pci_unregister_driver(struct pci_driver *drv);
extern int pci_enable_device(struct pci_dev *dev);
extern void pci_disable_device(struct pci_dev *dev);
extern void pci_set_master(struct pci_dev *dev);
extern int pci_request_regions(struct pci_dev *dev, const char *name);
extern void pci_release_regions(struct pci_dev *dev);
extern void *pci_ioremap_bar(struct pci_dev *dev, int bar);
extern int pci_read_config_byte(const struct pci_dev *dev, int where, u8 *val);
extern int pci_read_config_word(const struct pci_dev *dev, int where, u16 *val);
extern int pci_read_config_dword(const struct pci_dev *dev, int where, u32 *val);
extern int pci_write_config_byte(const struct pci_dev *dev, int where, u8 val);
extern int pci_write_config_word(const struct pci_dev *dev, int where, u16 val);
extern int pci_write_config_dword(const struct pci_dev *dev, int where, u32 val);
#define PCI_DEVICE(vend, dev) .vendor = (vend), .device = (dev)
#define module_pci_driver(drv)
"#,
        );

        // linux/slab.h - Memory allocation
        self.header_stubs.add(
            "linux/slab.h",
            r#"
#define GFP_KERNEL 0
#define GFP_ATOMIC 1
#define GFP_DMA 2
#define GFP_NOWAIT 4
extern void *kmalloc(size_t size, gfp_t flags);
extern void *kzalloc(size_t size, gfp_t flags);
extern void *kcalloc(size_t n, size_t size, gfp_t flags);
extern void *krealloc(void *p, size_t new_size, gfp_t flags);
extern void kfree(const void *objp);
extern void *vmalloc(unsigned long size);
extern void vfree(const void *addr);
extern void *kvmalloc(size_t size, gfp_t flags);
extern void kvfree(const void *addr);
extern struct kmem_cache *kmem_cache_create(const char *name, size_t size, size_t align, unsigned long flags, void (*ctor)(void *));
extern void kmem_cache_destroy(struct kmem_cache *s);
extern void *kmem_cache_alloc(struct kmem_cache *s, gfp_t flags);
extern void kmem_cache_free(struct kmem_cache *s, void *objp);
"#,
        );

        // linux/mutex.h - Mutex primitives
        self.header_stubs.add(
            "linux/mutex.h",
            r#"
struct mutex {
    int count;
};
#define DEFINE_MUTEX(name) struct mutex name = { .count = 1 }
#define mutex_init(mutex) do { (mutex)->count = 1; } while(0)
extern void mutex_lock(struct mutex *lock);
extern int mutex_trylock(struct mutex *lock);
extern void mutex_unlock(struct mutex *lock);
extern int mutex_is_locked(struct mutex *lock);
"#,
        );

        // linux/spinlock.h - Spinlock primitives
        self.header_stubs.add(
            "linux/spinlock.h",
            r#"
typedef struct {
    int lock;
} spinlock_t;
typedef struct {
    int lock;
} rwlock_t;
#define DEFINE_SPINLOCK(name) spinlock_t name = { .lock = 0 }
#define DEFINE_RWLOCK(name) rwlock_t name = { .lock = 0 }
#define spin_lock_init(lock) do { (lock)->lock = 0; } while(0)
extern void spin_lock(spinlock_t *lock);
extern void spin_unlock(spinlock_t *lock);
extern void spin_lock_irq(spinlock_t *lock);
extern void spin_unlock_irq(spinlock_t *lock);
extern void spin_lock_irqsave(spinlock_t *lock, unsigned long flags);
extern void spin_unlock_irqrestore(spinlock_t *lock, unsigned long flags);
extern int spin_trylock(spinlock_t *lock);
extern void read_lock(rwlock_t *lock);
extern void read_unlock(rwlock_t *lock);
extern void write_lock(rwlock_t *lock);
extern void write_unlock(rwlock_t *lock);
"#,
        );

        // linux/wait.h - Wait queues
        self.header_stubs.add(
            "linux/wait.h",
            r#"
struct wait_queue_head {
    spinlock_t lock;
};
typedef struct wait_queue_head wait_queue_head_t;
#define DECLARE_WAIT_QUEUE_HEAD(name) wait_queue_head_t name
#define init_waitqueue_head(wq) do { } while(0)
extern void wake_up(wait_queue_head_t *wq);
extern void wake_up_interruptible(wait_queue_head_t *wq);
extern void wake_up_all(wait_queue_head_t *wq);
#define wait_event(wq, condition) do { } while(0)
#define wait_event_interruptible(wq, condition) 0
#define wait_event_timeout(wq, condition, timeout) 0
"#,
        );

        // linux/interrupt.h - Interrupt handling
        self.header_stubs.add(
            "linux/interrupt.h",
            r#"
typedef int irqreturn_t;
#define IRQ_NONE 0
#define IRQ_HANDLED 1
#define IRQ_WAKE_THREAD 2
#define IRQF_SHARED 0x00000080
#define IRQF_TRIGGER_RISING 0x00000001
#define IRQF_TRIGGER_FALLING 0x00000002
typedef irqreturn_t (*irq_handler_t)(int, void *);
extern int request_irq(unsigned int irq, irq_handler_t handler, unsigned long flags, const char *name, void *dev);
extern void free_irq(unsigned int irq, void *dev_id);
extern int request_threaded_irq(unsigned int irq, irq_handler_t handler, irq_handler_t thread_fn, unsigned long flags, const char *name, void *dev);
extern void disable_irq(unsigned int irq);
extern void enable_irq(unsigned int irq);
extern void local_irq_disable(void);
extern void local_irq_enable(void);
"#,
        );

        // linux/dma-mapping.h - DMA operations
        self.header_stubs.add(
            "linux/dma-mapping.h",
            r#"
enum dma_data_direction {
    DMA_BIDIRECTIONAL = 0,
    DMA_TO_DEVICE = 1,
    DMA_FROM_DEVICE = 2,
    DMA_NONE = 3,
};
extern void *dma_alloc_coherent(struct device *dev, size_t size, dma_addr_t *dma_handle, gfp_t flag);
extern void dma_free_coherent(struct device *dev, size_t size, void *cpu_addr, dma_addr_t dma_handle);
extern dma_addr_t dma_map_single(struct device *dev, void *cpu_addr, size_t size, enum dma_data_direction dir);
extern void dma_unmap_single(struct device *dev, dma_addr_t addr, size_t size, enum dma_data_direction dir);
extern int dma_set_mask(struct device *dev, u64 mask);
extern int dma_set_coherent_mask(struct device *dev, u64 mask);
#define DMA_BIT_MASK(n) (((n) == 64) ? ~0ULL : ((1ULL << (n)) - 1))
"#,
        );

        // linux/io.h - Memory-mapped I/O
        self.header_stubs.add(
            "linux/io.h",
            r#"
extern void *ioremap(phys_addr_t offset, size_t size);
extern void iounmap(void *addr);
extern u8 readb(const volatile void *addr);
extern u16 readw(const volatile void *addr);
extern u32 readl(const volatile void *addr);
extern u64 readq(const volatile void *addr);
extern void writeb(u8 value, volatile void *addr);
extern void writew(u16 value, volatile void *addr);
extern void writel(u32 value, volatile void *addr);
extern void writeq(u64 value, volatile void *addr);
extern u8 ioread8(const void *addr);
extern u16 ioread16(const void *addr);
extern u32 ioread32(const void *addr);
extern void iowrite8(u8 value, void *addr);
extern void iowrite16(u16 value, void *addr);
extern void iowrite32(u32 value, void *addr);
"#,
        );

        // linux/errno.h - Error codes
        self.header_stubs.add(
            "linux/errno.h",
            r#"
#define EPERM 1
#define ENOENT 2
#define EIO 5
#define ENXIO 6
#define ENOMEM 12
#define EACCES 13
#define EFAULT 14
#define EBUSY 16
#define EEXIST 17
#define ENODEV 19
#define EINVAL 22
#define ENOSPC 28
#define ERANGE 34
#define EOPNOTSUPP 95
#define ETIMEDOUT 110
#define ERESTARTSYS 512
"#,
        );

        // linux/netdevice.h - Network device operations
        self.header_stubs.add(
            "linux/netdevice.h",
            r#"
struct net_device;
struct sk_buff;
struct net_device_stats;
typedef u16 netdev_features_t;
struct net_device_ops {
    int (*ndo_open)(struct net_device *dev);
    int (*ndo_stop)(struct net_device *dev);
    int (*ndo_start_xmit)(struct sk_buff *skb, struct net_device *dev);
    void (*ndo_set_rx_mode)(struct net_device *dev);
    int (*ndo_set_mac_address)(struct net_device *dev, void *addr);
    int (*ndo_validate_addr)(struct net_device *dev);
    int (*ndo_do_ioctl)(struct net_device *dev, struct ifreq *ifr, int cmd);
    int (*ndo_change_mtu)(struct net_device *dev, int new_mtu);
    void (*ndo_tx_timeout)(struct net_device *dev, unsigned int txqueue);
    struct net_device_stats *(*ndo_get_stats)(struct net_device *dev);
};
struct ethtool_ops {
    int (*get_link)(struct net_device *dev);
    int (*get_link_ksettings)(struct net_device *dev, struct ethtool_link_ksettings *cmd);
    int (*set_link_ksettings)(struct net_device *dev, const struct ethtool_link_ksettings *cmd);
};
extern struct net_device *alloc_etherdev(int sizeof_priv);
extern void free_netdev(struct net_device *dev);
extern int register_netdev(struct net_device *dev);
extern void unregister_netdev(struct net_device *dev);
extern void *netdev_priv(const struct net_device *dev);
"#,
        );

        // linux/uaccess.h - User space access
        self.header_stubs.add(
            "linux/uaccess.h",
            r#"
extern unsigned long copy_from_user(void *to, const void *from, unsigned long n);
extern unsigned long copy_to_user(void *to, const void *from, unsigned long n);
extern int get_user(int x, int *ptr);
extern int put_user(int x, int *ptr);
extern int access_ok(int type, const void *addr, unsigned long size);
#define VERIFY_READ 0
#define VERIFY_WRITE 1
"#,
        );

        // linux/string.h - String operations
        self.header_stubs.add(
            "linux/string.h",
            r#"
extern void *memset(void *s, int c, size_t n);
extern void *memcpy(void *dest, const void *src, size_t n);
extern void *memmove(void *dest, const void *src, size_t n);
extern int memcmp(const void *s1, const void *s2, size_t n);
extern size_t strlen(const char *s);
extern char *strcpy(char *dest, const char *src);
extern char *strncpy(char *dest, const char *src, size_t n);
extern int strcmp(const char *s1, const char *s2);
extern int strncmp(const char *s1, const char *s2, size_t n);
extern char *strcat(char *dest, const char *src);
extern char *strchr(const char *s, int c);
extern char *strstr(const char *haystack, const char *needle);
"#,
        );

        // linux/workqueue.h - Work queues
        self.header_stubs.add(
            "linux/workqueue.h",
            r#"
struct work_struct;
struct delayed_work;
struct workqueue_struct;
typedef void (*work_func_t)(struct work_struct *work);
#define DECLARE_WORK(n, f) struct work_struct n
#define DECLARE_DELAYED_WORK(n, f) struct delayed_work n
#define INIT_WORK(work, func) do { } while(0)
#define INIT_DELAYED_WORK(dwork, func) do { } while(0)
extern struct workqueue_struct *create_workqueue(const char *name);
extern void destroy_workqueue(struct workqueue_struct *wq);
extern int queue_work(struct workqueue_struct *wq, struct work_struct *work);
extern int queue_delayed_work(struct workqueue_struct *wq, struct delayed_work *dwork, unsigned long delay);
extern int schedule_work(struct work_struct *work);
extern int schedule_delayed_work(struct delayed_work *dwork, unsigned long delay);
extern int cancel_work_sync(struct work_struct *work);
extern int cancel_delayed_work_sync(struct delayed_work *dwork);
extern void flush_workqueue(struct workqueue_struct *wq);
"#,
        );

        // linux/timer.h - Timer operations
        self.header_stubs.add(
            "linux/timer.h",
            r#"
struct timer_list {
    unsigned long expires;
    void (*function)(struct timer_list *);
    unsigned long data;
};
#define DEFINE_TIMER(name, func) struct timer_list name
extern void timer_setup(struct timer_list *timer, void (*callback)(struct timer_list *), unsigned int flags);
extern int mod_timer(struct timer_list *timer, unsigned long expires);
extern int del_timer(struct timer_list *timer);
extern int del_timer_sync(struct timer_list *timer);
extern unsigned long jiffies;
#define HZ 100
#define msecs_to_jiffies(m) ((m) * HZ / 1000)
"#,
        );

        // linux/completion.h - Completion mechanism
        self.header_stubs.add(
            "linux/completion.h",
            r#"
struct completion {
    unsigned int done;
    wait_queue_head_t wait;
};
#define DECLARE_COMPLETION(name) struct completion name
#define init_completion(x) do { (x)->done = 0; } while(0)
extern void wait_for_completion(struct completion *x);
extern int wait_for_completion_timeout(struct completion *x, unsigned long timeout);
extern int wait_for_completion_interruptible(struct completion *x);
extern void complete(struct completion *x);
extern void complete_all(struct completion *x);
extern void reinit_completion(struct completion *x);
"#,
        );

        // linux/atomic.h - Atomic operations
        self.header_stubs.add(
            "linux/atomic.h",
            r#"
typedef struct {
    int counter;
} atomic_t;
typedef struct {
    long long counter;
} atomic64_t;
#define ATOMIC_INIT(i) { (i) }
#define atomic_read(v) ((v)->counter)
#define atomic_set(v, i) ((v)->counter = (i))
extern void atomic_inc(atomic_t *v);
extern void atomic_dec(atomic_t *v);
extern int atomic_inc_return(atomic_t *v);
extern int atomic_dec_return(atomic_t *v);
extern int atomic_dec_and_test(atomic_t *v);
extern int atomic_add_return(int i, atomic_t *v);
extern int atomic_sub_return(int i, atomic_t *v);
extern int atomic_cmpxchg(atomic_t *v, int old, int new);
"#,
        );

        // linux/list.h - Linked list operations
        self.header_stubs.add(
            "linux/list.h",
            r#"
struct list_head {
    struct list_head *next, *prev;
};
#define LIST_HEAD_INIT(name) { &(name), &(name) }
#define LIST_HEAD(name) struct list_head name = LIST_HEAD_INIT(name)
#define INIT_LIST_HEAD(ptr) do { (ptr)->next = (ptr); (ptr)->prev = (ptr); } while (0)
extern void list_add(struct list_head *new, struct list_head *head);
extern void list_add_tail(struct list_head *new, struct list_head *head);
extern void list_del(struct list_head *entry);
extern void list_del_init(struct list_head *entry);
extern int list_empty(const struct list_head *head);
#define list_entry(ptr, type, member) container_of(ptr, type, member)
#define list_for_each(pos, head) for (pos = (head)->next; pos != (head); pos = pos->next)
#define list_for_each_safe(pos, n, head) for (pos = (head)->next, n = pos->next; pos != (head); pos = n, n = pos->next)
"#,
        );
    }

    fn init_ops_structs(&mut self) {
        // file_operations
        self.ops_structs.push(OpsStructDef {
            struct_name: "file_operations".to_string(),
            fields: vec![
                OpsFieldDef {
                    name: "open".to_string(),
                    category: CallbackCategory::Open,
                },
                OpsFieldDef {
                    name: "release".to_string(),
                    category: CallbackCategory::Close,
                },
                OpsFieldDef {
                    name: "read".to_string(),
                    category: CallbackCategory::Read,
                },
                OpsFieldDef {
                    name: "write".to_string(),
                    category: CallbackCategory::Write,
                },
                OpsFieldDef {
                    name: "unlocked_ioctl".to_string(),
                    category: CallbackCategory::Ioctl,
                },
                OpsFieldDef {
                    name: "compat_ioctl".to_string(),
                    category: CallbackCategory::Ioctl,
                },
                OpsFieldDef {
                    name: "mmap".to_string(),
                    category: CallbackCategory::Mmap,
                },
                OpsFieldDef {
                    name: "poll".to_string(),
                    category: CallbackCategory::Poll,
                },
                OpsFieldDef {
                    name: "llseek".to_string(),
                    category: CallbackCategory::Other,
                },
                OpsFieldDef {
                    name: "fsync".to_string(),
                    category: CallbackCategory::Other,
                },
            ],
        });

        // pci_driver
        self.ops_structs.push(OpsStructDef {
            struct_name: "pci_driver".to_string(),
            fields: vec![
                OpsFieldDef {
                    name: "probe".to_string(),
                    category: CallbackCategory::Probe,
                },
                OpsFieldDef {
                    name: "remove".to_string(),
                    category: CallbackCategory::Remove,
                },
                OpsFieldDef {
                    name: "suspend".to_string(),
                    category: CallbackCategory::Suspend,
                },
                OpsFieldDef {
                    name: "resume".to_string(),
                    category: CallbackCategory::Resume,
                },
                OpsFieldDef {
                    name: "shutdown".to_string(),
                    category: CallbackCategory::Cleanup,
                },
            ],
        });

        // net_device_ops
        self.ops_structs.push(OpsStructDef {
            struct_name: "net_device_ops".to_string(),
            fields: vec![
                OpsFieldDef {
                    name: "ndo_open".to_string(),
                    category: CallbackCategory::Open,
                },
                OpsFieldDef {
                    name: "ndo_stop".to_string(),
                    category: CallbackCategory::Close,
                },
                OpsFieldDef {
                    name: "ndo_start_xmit".to_string(),
                    category: CallbackCategory::Write,
                },
                OpsFieldDef {
                    name: "ndo_set_rx_mode".to_string(),
                    category: CallbackCategory::Other,
                },
                OpsFieldDef {
                    name: "ndo_set_mac_address".to_string(),
                    category: CallbackCategory::Other,
                },
                OpsFieldDef {
                    name: "ndo_do_ioctl".to_string(),
                    category: CallbackCategory::Ioctl,
                },
                OpsFieldDef {
                    name: "ndo_tx_timeout".to_string(),
                    category: CallbackCategory::Timer,
                },
            ],
        });

        // platform_driver
        self.ops_structs.push(OpsStructDef {
            struct_name: "platform_driver".to_string(),
            fields: vec![
                OpsFieldDef {
                    name: "probe".to_string(),
                    category: CallbackCategory::Probe,
                },
                OpsFieldDef {
                    name: "remove".to_string(),
                    category: CallbackCategory::Remove,
                },
                OpsFieldDef {
                    name: "suspend".to_string(),
                    category: CallbackCategory::Suspend,
                },
                OpsFieldDef {
                    name: "resume".to_string(),
                    category: CallbackCategory::Resume,
                },
            ],
        });
    }

    fn init_call_normalizations(&mut self) {
        // Memory allocation
        self.call_normalizations.insert("kmalloc", "MemAlloc");
        self.call_normalizations.insert("kzalloc", "MemAlloc");
        self.call_normalizations.insert("kcalloc", "MemAlloc");
        self.call_normalizations.insert("krealloc", "MemRealloc");
        self.call_normalizations.insert("vmalloc", "MemAlloc");
        self.call_normalizations.insert("kvmalloc", "MemAlloc");
        self.call_normalizations
            .insert("kmem_cache_alloc", "MemAlloc");
        self.call_normalizations
            .insert("dma_alloc_coherent", "DmaAlloc");

        // Memory free
        self.call_normalizations.insert("kfree", "MemFree");
        self.call_normalizations.insert("vfree", "MemFree");
        self.call_normalizations.insert("kvfree", "MemFree");
        self.call_normalizations
            .insert("kmem_cache_free", "MemFree");
        self.call_normalizations
            .insert("dma_free_coherent", "DmaFree");

        // Memory operations
        self.call_normalizations.insert("memcpy", "MemCopy");
        self.call_normalizations.insert("memmove", "MemCopy");
        self.call_normalizations.insert("memset", "MemSet");

        // User/kernel boundary
        self.call_normalizations
            .insert("copy_from_user", "CopyFromUser");
        self.call_normalizations.insert("get_user", "CopyFromUser");
        self.call_normalizations
            .insert("copy_to_user", "CopyToUser");
        self.call_normalizations.insert("put_user", "CopyToUser");

        // Locking
        self.call_normalizations.insert("mutex_lock", "LockAcquire");
        self.call_normalizations.insert("spin_lock", "LockAcquire");
        self.call_normalizations
            .insert("spin_lock_irq", "LockAcquire");
        self.call_normalizations
            .insert("spin_lock_irqsave", "LockAcquire");
        self.call_normalizations.insert("read_lock", "LockAcquire");
        self.call_normalizations.insert("write_lock", "LockAcquire");
        self.call_normalizations
            .insert("mutex_unlock", "LockRelease");
        self.call_normalizations
            .insert("spin_unlock", "LockRelease");
        self.call_normalizations
            .insert("spin_unlock_irq", "LockRelease");
        self.call_normalizations
            .insert("spin_unlock_irqrestore", "LockRelease");
        self.call_normalizations
            .insert("read_unlock", "LockRelease");
        self.call_normalizations
            .insert("write_unlock", "LockRelease");

        // Wait/signal
        self.call_normalizations
            .insert("wait_for_completion", "WaitEvent");
        self.call_normalizations
            .insert("wait_event_interruptible", "WaitEvent");
        self.call_normalizations.insert("complete", "SignalEvent");
        self.call_normalizations
            .insert("complete_all", "SignalEvent");
        self.call_normalizations.insert("wake_up", "SignalEvent");
        self.call_normalizations
            .insert("wake_up_interruptible", "SignalEvent");

        // I/O mapping
        self.call_normalizations.insert("ioremap", "IoRemap");
        self.call_normalizations
            .insert("pci_ioremap_bar", "IoRemap");
        self.call_normalizations.insert("iounmap", "IoUnmap");

        // I/O operations
        self.call_normalizations.insert("readb", "IoRead");
        self.call_normalizations.insert("readw", "IoRead");
        self.call_normalizations.insert("readl", "IoRead");
        self.call_normalizations.insert("readq", "IoRead");
        self.call_normalizations.insert("ioread8", "IoRead");
        self.call_normalizations.insert("ioread16", "IoRead");
        self.call_normalizations.insert("ioread32", "IoRead");
        self.call_normalizations.insert("writeb", "IoWrite");
        self.call_normalizations.insert("writew", "IoWrite");
        self.call_normalizations.insert("writel", "IoWrite");
        self.call_normalizations.insert("writeq", "IoWrite");
        self.call_normalizations.insert("iowrite8", "IoWrite");
        self.call_normalizations.insert("iowrite16", "IoWrite");
        self.call_normalizations.insert("iowrite32", "IoWrite");

        // DMA mapping
        self.call_normalizations.insert("dma_map_single", "DmaMap");
        self.call_normalizations
            .insert("dma_unmap_single", "DmaUnmap");

        // Interrupts
        self.call_normalizations
            .insert("request_irq", "InterruptRegister");
        self.call_normalizations
            .insert("request_threaded_irq", "InterruptRegister");
        self.call_normalizations
            .insert("free_irq", "InterruptUnregister");
        self.call_normalizations
            .insert("disable_irq", "InterruptDisable");
        self.call_normalizations
            .insert("enable_irq", "InterruptEnable");

        // Device registration
        self.call_normalizations
            .insert("pci_register_driver", "DeviceRegister");
        self.call_normalizations
            .insert("register_netdev", "DeviceRegister");
        self.call_normalizations
            .insert("register_chrdev", "DeviceRegister");
        self.call_normalizations
            .insert("device_register", "DeviceRegister");
        self.call_normalizations
            .insert("pci_unregister_driver", "DeviceUnregister");
        self.call_normalizations
            .insert("unregister_netdev", "DeviceUnregister");
        self.call_normalizations
            .insert("unregister_chrdev", "DeviceUnregister");
        self.call_normalizations
            .insert("device_unregister", "DeviceUnregister");

        // Logging
        self.call_normalizations.insert("printk", "Log");
        self.call_normalizations.insert("pr_info", "Log");
        self.call_normalizations.insert("pr_err", "Log");
        self.call_normalizations.insert("pr_warn", "Log");
        self.call_normalizations.insert("pr_debug", "Log");
        self.call_normalizations.insert("dev_info", "Log");
        self.call_normalizations.insert("dev_err", "Log");
        self.call_normalizations.insert("dev_warn", "Log");
        self.call_normalizations.insert("dev_dbg", "Log");
    }
}

impl Default for LinuxPlatform {
    fn default() -> Self {
        Self::new()
    }
}

impl PlatformModule for LinuxPlatform {
    fn id(&self) -> &'static str {
        "linux"
    }

    fn name(&self) -> &'static str {
        "Linux Kernel"
    }

    fn detection_patterns(&self) -> Vec<DetectionPattern> {
        vec![
            // Include patterns - high weight
            DetectionPattern::include("linux/", 3.0),
            DetectionPattern::include("asm/", 1.5),
            DetectionPattern::include("uapi/", 1.5),
            // Macro patterns - very high weight for module macros
            DetectionPattern::macro_pattern("MODULE_LICENSE", 4.0),
            DetectionPattern::macro_pattern("MODULE_AUTHOR", 2.0),
            DetectionPattern::macro_pattern("MODULE_DESCRIPTION", 2.0),
            DetectionPattern::macro_pattern("module_init", 3.0),
            DetectionPattern::macro_pattern("module_exit", 3.0),
            DetectionPattern::macro_pattern("EXPORT_SYMBOL", 2.5),
            DetectionPattern::macro_pattern("EXPORT_SYMBOL_GPL", 2.5),
            DetectionPattern::macro_pattern("__init", 2.0),
            DetectionPattern::macro_pattern("__exit", 2.0),
            DetectionPattern::macro_pattern("CONFIG_", 1.0),
            DetectionPattern::macro_pattern("KERN_INFO", 1.5),
            DetectionPattern::macro_pattern("KERN_ERR", 1.5),
            DetectionPattern::macro_pattern("GFP_KERNEL", 2.0),
            // Function patterns
            DetectionPattern::function_call("printk", 2.0),
            DetectionPattern::function_call("kmalloc", 2.0),
            DetectionPattern::function_call("kfree", 2.0),
            DetectionPattern::function_call("kzalloc", 2.0),
            DetectionPattern::function_call("copy_from_user", 2.5),
            DetectionPattern::function_call("copy_to_user", 2.5),
            // Type patterns
            DetectionPattern::type_name("spinlock_t", 1.5),
            DetectionPattern::type_name("atomic_t", 1.5),
            DetectionPattern::type_name("wait_queue_head_t", 1.5),
        ]
    }

    fn header_stubs(&self) -> &HeaderStubs {
        &self.header_stubs
    }

    fn attributes_to_strip(&self) -> &[&'static str] {
        &[
            // Section/init attributes
            "__init",
            "__exit",
            "__initdata",
            "__exitdata",
            "__initconst",
            "__devinit",
            "__devexit",
            // Compiler hints
            "__cold",
            "__hot",
            "__pure",
            "__const",
            "__noreturn",
            "__malloc",
            "__weak",
            "__alias",
            "__always_inline",
            "__noinline",
            "noinline",
            "inline",
            "__inline",
            "__inline__",
            "__section",
            "__visible",
            "__flatten",
            // Address space annotations
            "__user",
            "__kernel",
            "__iomem",
            "__percpu",
            "__rcu",
            "__force",
            "__bitwise",
            "__safe",
            // Unused/maybe annotations
            "__maybe_unused",
            "__always_unused",
            "__unused",
            // Packing and alignment
            "__packed",
            "__aligned",
            "__cacheline_aligned",
            "__cacheline_aligned_in_smp",
            "__page_aligned_data",
            "__page_aligned_bss",
            // Deprecation
            "__deprecated",
            "__deprecated_for_modules",
            // Locking annotations
            "__must_check",
            "__must_hold",
            "__acquires",
            "__releases",
            "__acquire",
            "__release",
            "__cond_lock",
            // Memory placement
            "__read_mostly",
            "__ro_after_init",
            // Calling conventions
            "asmlinkage",
            "fastcall",
            "regparm",
        ]
    }

    fn ops_structs(&self) -> &[OpsStructDef] {
        &self.ops_structs
    }

    fn call_normalizations(&self) -> &HashMap<&'static str, &'static str> {
        &self.call_normalizations
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::DetectionKind;

    #[test]
    fn test_linux_platform_id() {
        let platform = LinuxPlatform::new();
        assert_eq!(platform.id(), "linux");
        assert_eq!(platform.name(), "Linux Kernel");
    }

    #[test]
    fn test_linux_header_stubs() {
        let platform = LinuxPlatform::new();
        let stubs = platform.header_stubs();

        assert!(stubs.has_stub("linux/types.h"));
        assert!(stubs.has_stub("linux/kernel.h"));
        assert!(stubs.has_stub("linux/module.h"));
        assert!(stubs.has_stub("linux/fs.h"));
        assert!(stubs.has_stub("linux/pci.h"));
        assert!(stubs.has_stub("linux/slab.h"));
    }

    #[test]
    fn test_linux_stubs_content() {
        let platform = LinuxPlatform::new();
        let stubs = platform.header_stubs();

        let source = "#include <linux/types.h>\n#include <linux/kernel.h>";
        let stub_content = stubs.get_for_includes(source);

        // Should contain type definitions from linux/types.h
        assert!(stub_content.contains("typedef unsigned int u32"));
        assert!(stub_content.contains("typedef unsigned long long u64"));

        // Should contain definitions from linux/kernel.h
        assert!(stub_content.contains("extern int printk"));
    }

    #[test]
    fn test_linux_detection_patterns() {
        let platform = LinuxPlatform::new();
        let patterns = platform.detection_patterns();

        // Should have include patterns
        let include_patterns: Vec<_> = patterns
            .iter()
            .filter(|p| p.kind == DetectionKind::Include)
            .collect();
        assert!(!include_patterns.is_empty());

        // Should have macro patterns
        let macro_patterns: Vec<_> = patterns
            .iter()
            .filter(|p| p.kind == DetectionKind::Macro)
            .collect();
        assert!(!macro_patterns.is_empty());

        // MODULE_LICENSE should have high weight
        let module_license = patterns
            .iter()
            .find(|p| p.pattern == "MODULE_LICENSE")
            .unwrap();
        assert!(module_license.weight >= 3.0);
    }

    #[test]
    fn test_linux_attributes_to_strip() {
        let platform = LinuxPlatform::new();
        let attrs = platform.attributes_to_strip();

        assert!(attrs.contains(&"__init"));
        assert!(attrs.contains(&"__exit"));
        assert!(attrs.contains(&"__user"));
        assert!(attrs.contains(&"__iomem"));
        assert!(attrs.contains(&"__must_check"));
    }

    #[test]
    fn test_linux_ops_structs() {
        let platform = LinuxPlatform::new();
        let ops = platform.ops_structs();

        // Should have file_operations
        let file_ops = ops.iter().find(|o| o.struct_name == "file_operations");
        assert!(file_ops.is_some());
        let file_ops = file_ops.unwrap();

        // Check fields
        let open_field = file_ops.fields.iter().find(|f| f.name == "open");
        assert!(open_field.is_some());
        assert_eq!(open_field.unwrap().category, CallbackCategory::Open);

        // Should have pci_driver
        let pci_driver = ops.iter().find(|o| o.struct_name == "pci_driver");
        assert!(pci_driver.is_some());
    }

    #[test]
    fn test_linux_call_normalizations() {
        let platform = LinuxPlatform::new();
        let norms = platform.call_normalizations();

        assert_eq!(norms.get("kmalloc"), Some(&"MemAlloc"));
        assert_eq!(norms.get("kfree"), Some(&"MemFree"));
        assert_eq!(norms.get("copy_from_user"), Some(&"CopyFromUser"));
        assert_eq!(norms.get("mutex_lock"), Some(&"LockAcquire"));
        assert_eq!(norms.get("printk"), Some(&"Log"));
    }
}
