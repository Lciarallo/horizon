/* Prints the C ABI facts of devkitA64 newlib + libnx that the libc crate must match.
 * Compiled with -S only (it never runs): each value becomes `.quad` data that
 * tools/abi_dump.sh reads back. */
#include <stddef.h>
#include <stdint.h>
#include <errno.h>
#include <fcntl.h>
#include <dirent.h>
#include <poll.h>
#include <pthread.h>
#include <sched.h>
#include <signal.h>
#include <time.h>
#include <unistd.h>
#include <netdb.h>
#include <sys/stat.h>
#include <sys/time.h>
#include <sys/types.h>
#include <sys/socket.h>
#include <sys/ioctl.h>
#include <sys/select.h>

#include <netinet/in.h>
#include <netinet/tcp.h>
#include <arpa/inet.h>

#define V(name, expr) const long long abi_##name = (long long)(expr);
#define SZ(t, n) V(sizeof_##n, sizeof(t)) V(alignof_##n, _Alignof(t))
#define OFF(t, f, n) V(offsetof_##n##_##f, offsetof(t, f))

SZ(struct stat, stat)
OFF(struct stat, st_dev, stat) OFF(struct stat, st_ino, stat) OFF(struct stat, st_mode, stat)
OFF(struct stat, st_nlink, stat) OFF(struct stat, st_uid, stat) OFF(struct stat, st_gid, stat)
OFF(struct stat, st_rdev, stat) OFF(struct stat, st_size, stat) OFF(struct stat, st_atim, stat)
OFF(struct stat, st_mtim, stat) OFF(struct stat, st_ctim, stat) OFF(struct stat, st_blksize, stat)
OFF(struct stat, st_blocks, stat)
SZ(dev_t, dev_t) SZ(ino_t, ino_t) SZ(mode_t, mode_t) SZ(nlink_t, nlink_t) SZ(uid_t, uid_t)
SZ(gid_t, gid_t) SZ(off_t, off_t) SZ(blksize_t, blksize_t) SZ(blkcnt_t, blkcnt_t) SZ(time_t, time_t)
SZ(clockid_t, clockid_t) SZ(clock_t, clock_t) SZ(suseconds_t, suseconds_t) SZ(ssize_t, ssize_t)
SZ(socklen_t, socklen_t) SZ(sa_family_t, sa_family_t) SZ(in_port_t, in_port_t) SZ(nfds_t, nfds_t)
SZ(wchar_t, wchar_t) SZ(sigset_t, sigset_t) SZ(useconds_t, useconds_t) SZ(pid_t, pid_t)
SZ(struct timespec, timespec) SZ(struct timeval, timeval) OFF(struct timeval, tv_usec, timeval)
SZ(struct dirent, dirent) OFF(struct dirent, d_ino, dirent) OFF(struct dirent, d_name, dirent)
#ifdef _DIRENT_HAVE_D_TYPE
OFF(struct dirent, d_type, dirent)
#endif
V(sizeof_d_name, sizeof(((struct dirent*)0)->d_name))
SZ(pthread_t, pthread_t) SZ(pthread_attr_t, pthread_attr_t) SZ(pthread_mutex_t, pthread_mutex_t)
SZ(pthread_mutexattr_t, pthread_mutexattr_t) SZ(pthread_cond_t, pthread_cond_t)
SZ(pthread_condattr_t, pthread_condattr_t) SZ(pthread_rwlock_t, pthread_rwlock_t)
SZ(pthread_rwlockattr_t, pthread_rwlockattr_t) SZ(pthread_key_t, pthread_key_t)
SZ(pthread_once_t, pthread_once_t) SZ(struct sched_param, sched_param)
SZ(struct sockaddr, sockaddr) OFF(struct sockaddr, sa_family, sockaddr) OFF(struct sockaddr, sa_data, sockaddr)
SZ(struct sockaddr_storage, sockaddr_storage) OFF(struct sockaddr_storage, ss_family, sockaddr_storage)
SZ(struct sockaddr_in, sockaddr_in) OFF(struct sockaddr_in, sin_family, sockaddr_in)
OFF(struct sockaddr_in, sin_port, sockaddr_in) OFF(struct sockaddr_in, sin_addr, sockaddr_in)
SZ(struct sockaddr_in6, sockaddr_in6) OFF(struct sockaddr_in6, sin6_family, sockaddr_in6)
OFF(struct sockaddr_in6, sin6_port, sockaddr_in6) OFF(struct sockaddr_in6, sin6_flowinfo, sockaddr_in6)
OFF(struct sockaddr_in6, sin6_addr, sockaddr_in6) OFF(struct sockaddr_in6, sin6_scope_id, sockaddr_in6)
SZ(struct in_addr, in_addr) SZ(struct in6_addr, in6_addr)
SZ(struct addrinfo, addrinfo) OFF(struct addrinfo, ai_flags, addrinfo) OFF(struct addrinfo, ai_family, addrinfo)
OFF(struct addrinfo, ai_socktype, addrinfo) OFF(struct addrinfo, ai_protocol, addrinfo)
OFF(struct addrinfo, ai_addrlen, addrinfo) OFF(struct addrinfo, ai_canonname, addrinfo)
OFF(struct addrinfo, ai_addr, addrinfo) OFF(struct addrinfo, ai_next, addrinfo)
SZ(struct pollfd, pollfd) OFF(struct pollfd, events, pollfd) OFF(struct pollfd, revents, pollfd)
SZ(struct linger, linger) SZ(struct ip_mreq, ip_mreq)
SZ(struct hostent, hostent) SZ(struct tm, tm) SZ(fd_set, fd_set) SZ(struct iovec, iovec)
SZ(struct msghdr, msghdr)
/* constants */
V(EPERM,EPERM) V(ENOENT,ENOENT) V(EINTR,EINTR) V(EIO,EIO) V(EBADF,EBADF) V(EAGAIN,EAGAIN)
V(EWOULDBLOCK,EWOULDBLOCK) V(ENOMEM,ENOMEM) V(EACCES,EACCES) V(EEXIST,EEXIST) V(EXDEV,EXDEV)
V(ENOTDIR,ENOTDIR) V(EISDIR,EISDIR) V(EINVAL,EINVAL) V(ENOSPC,ENOSPC) V(ESPIPE,ESPIPE)
V(EROFS,EROFS) V(EPIPE,EPIPE) V(ERANGE,ERANGE) V(EDEADLK,EDEADLK) V(ENAMETOOLONG,ENAMETOOLONG)
V(ENOSYS,ENOSYS) V(ENOTEMPTY,ENOTEMPTY) V(ELOOP,ELOOP) V(EINPROGRESS,EINPROGRESS)
V(EALREADY,EALREADY) V(ENOTSOCK,ENOTSOCK) V(EADDRINUSE,EADDRINUSE) V(EADDRNOTAVAIL,EADDRNOTAVAIL)
V(ENETDOWN,ENETDOWN) V(ENETUNREACH,ENETUNREACH) V(ECONNABORTED,ECONNABORTED) V(ECONNRESET,ECONNRESET)
V(ENOTCONN,ENOTCONN) V(ETIMEDOUT,ETIMEDOUT) V(ECONNREFUSED,ECONNREFUSED) V(EHOSTUNREACH,EHOSTUNREACH)
V(ENOTSUP,ENOTSUP) V(EOPNOTSUPP,EOPNOTSUPP) V(EMSGSIZE,EMSGSIZE) V(EFBIG,EFBIG) V(ETXTBSY,ETXTBSY)
V(EBUSY,EBUSY) V(EMLINK,EMLINK) V(EDQUOT,EDQUOT) V(ESTALE,ESTALE) V(EHOSTDOWN,EHOSTDOWN)
V(ENOBUFS,ENOBUFS) V(EPROTONOSUPPORT,EPROTONOSUPPORT) V(EAFNOSUPPORT,EAFNOSUPPORT) V(EISCONN,EISCONN)
V(O_RDONLY,O_RDONLY) V(O_WRONLY,O_WRONLY) V(O_RDWR,O_RDWR) V(O_APPEND,O_APPEND) V(O_CREAT,O_CREAT)
V(O_TRUNC,O_TRUNC) V(O_EXCL,O_EXCL) V(O_NONBLOCK,O_NONBLOCK) V(O_CLOEXEC,O_CLOEXEC)
V(O_DIRECTORY,O_DIRECTORY) V(O_NOFOLLOW,O_NOFOLLOW) V(O_ACCMODE,O_ACCMODE)
V(F_GETFD,F_GETFD) V(F_SETFD,F_SETFD) V(F_GETFL,F_GETFL) V(F_SETFL,F_SETFL) V(F_DUPFD_CLOEXEC,F_DUPFD_CLOEXEC) V(FD_CLOEXEC,FD_CLOEXEC)
V(S_IFMT,S_IFMT) V(S_IFDIR,S_IFDIR) V(S_IFREG,S_IFREG) V(S_IFLNK,S_IFLNK)
V(DT_UNKNOWN,DT_UNKNOWN) V(DT_DIR,DT_DIR) V(DT_REG,DT_REG) V(DT_LNK,DT_LNK)
V(SEEK_SET,SEEK_SET) V(SEEK_CUR,SEEK_CUR) V(SEEK_END,SEEK_END)
V(CLOCK_REALTIME,CLOCK_REALTIME) V(CLOCK_MONOTONIC,CLOCK_MONOTONIC)
V(_SC_PAGESIZE,_SC_PAGESIZE) V(_SC_NPROCESSORS_ONLN,_SC_NPROCESSORS_ONLN)
V(AF_UNSPEC,AF_UNSPEC) V(AF_INET,AF_INET) V(AF_INET6,AF_INET6) V(AF_UNIX,AF_UNIX)
V(SOCK_STREAM,SOCK_STREAM) V(SOCK_DGRAM,SOCK_DGRAM) V(SOCK_RAW,SOCK_RAW)
V(SOL_SOCKET,SOL_SOCKET) V(SO_REUSEADDR,SO_REUSEADDR) V(SO_KEEPALIVE,SO_KEEPALIVE) V(SO_BROADCAST,SO_BROADCAST)
V(SO_LINGER,SO_LINGER) V(SO_SNDBUF,SO_SNDBUF) V(SO_RCVBUF,SO_RCVBUF) V(SO_RCVTIMEO,SO_RCVTIMEO)
V(SO_SNDTIMEO,SO_SNDTIMEO) V(SO_ERROR,SO_ERROR) V(SO_TYPE,SO_TYPE)
V(IPPROTO_IP,IPPROTO_IP) V(IPPROTO_TCP,IPPROTO_TCP) V(IPPROTO_UDP,IPPROTO_UDP) V(IPPROTO_IPV6,IPPROTO_IPV6)
V(TCP_NODELAY,TCP_NODELAY) V(IP_TTL,IP_TTL) V(IP_MULTICAST_IF,IP_MULTICAST_IF) V(IP_MULTICAST_TTL,IP_MULTICAST_TTL)
V(IP_MULTICAST_LOOP,IP_MULTICAST_LOOP) V(IP_ADD_MEMBERSHIP,IP_ADD_MEMBERSHIP) V(IP_DROP_MEMBERSHIP,IP_DROP_MEMBERSHIP)
V(MSG_PEEK,MSG_PEEK) V(MSG_DONTWAIT,MSG_DONTWAIT) V(MSG_WAITALL,MSG_WAITALL) V(MSG_OOB,MSG_OOB)
#ifdef MSG_NOSIGNAL
V(MSG_NOSIGNAL,MSG_NOSIGNAL)
#endif
V(SHUT_RD,SHUT_RD) V(SHUT_WR,SHUT_WR) V(SHUT_RDWR,SHUT_RDWR)
V(POLLIN,POLLIN) V(POLLPRI,POLLPRI) V(POLLOUT,POLLOUT) V(POLLERR,POLLERR) V(POLLHUP,POLLHUP) V(POLLNVAL,POLLNVAL)
V(FIONBIO,FIONBIO) V(FIONREAD,FIONREAD)
V(EAI_AGAIN,EAI_AGAIN) V(EAI_FAIL,EAI_FAIL) V(EAI_NONAME,EAI_NONAME) V(EAI_SYSTEM,EAI_SYSTEM) V(EAI_SERVICE,EAI_SERVICE)
V(AI_PASSIVE,AI_PASSIVE) V(AI_CANONNAME,AI_CANONNAME) V(AI_NUMERICHOST,AI_NUMERICHOST)

V(PTHREAD_MUTEX_NORMAL,PTHREAD_MUTEX_NORMAL) V(PTHREAD_MUTEX_RECURSIVE,PTHREAD_MUTEX_RECURSIVE) V(PTHREAD_MUTEX_ERRORCHECK,PTHREAD_MUTEX_ERRORCHECK) V(PTHREAD_MUTEX_DEFAULT,PTHREAD_MUTEX_DEFAULT)
V(STDIN_FILENO,STDIN_FILENO) V(PATH_MAX_,4096)
V(SIGPIPE,SIGPIPE) V(SIG_SETMASK,SIG_SETMASK)
