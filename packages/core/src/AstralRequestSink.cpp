#include "AstralRequestSink.h"

#include <arpa/inet.h>
#include <atomic>
#include <mutex>
#include <netinet/in.h>
#include <sys/socket.h>
#include <thread>
#include <unistd.h>

namespace {

constexpr char kEmptyResponse[] = "HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";

std::atomic<uint16_t> gPort{0};
std::once_flag gStarted;

void answer(int client)
{
    char buffer[4096];
    timeval timeout{1, 0};
    setsockopt(client, SOL_SOCKET, SO_RCVTIMEO, &timeout, sizeof(timeout));
    while (recv(client, buffer, sizeof(buffer), MSG_DONTWAIT) > 0) {
    }
    send(client, kEmptyResponse, sizeof(kEmptyResponse) - 1, 0);
    close(client);
}

void serve(int listener)
{
    while (true) {
        int client = accept(listener, nullptr, nullptr);
        if (client < 0) continue;
        std::thread(answer, client).detach();
    }
}

void listenOnLoopback()
{
    int listener = socket(AF_INET, SOCK_STREAM, 0);
    if (listener < 0) return;
    sockaddr_in address{};
    address.sin_family = AF_INET;
    address.sin_port = 0;
    address.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
    socklen_t length = sizeof(address);
    if (bind(listener, reinterpret_cast<sockaddr *>(&address), sizeof(address)) != 0 || listen(listener, 16) != 0 ||
        getsockname(listener, reinterpret_cast<sockaddr *>(&address), &length) != 0) {
        close(listener);
        return;
    }
    gPort = ntohs(address.sin_port);
    std::thread(serve, listener).detach();
}

} // namespace

namespace astral::sink {

uint16_t start()
{
    std::call_once(gStarted, listenOnLoopback);
    return gPort;
}

std::string url()
{
    uint16_t port = start();
    return port ? "http://127.0.0.1:" + std::to_string(port) + "/" : std::string();
}

} // namespace astral::sink
