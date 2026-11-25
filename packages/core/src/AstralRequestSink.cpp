#include "AstralRequestSink.h"

#include <arpa/inet.h>
#include <atomic>
#include <map>
#include <mutex>
#include <string>
#include <netinet/in.h>
#include <sys/socket.h>
#include <thread>
#include <unistd.h>

namespace {

constexpr char kEmptyResponse[] = "HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";

std::atomic<uint16_t> gPort{0};
std::once_flag gStarted;
std::mutex gResponsesLock;
std::map<std::string, std::string> gResponses;
std::atomic<uint64_t> gNextToken{1};

std::string registeredResponse(const std::string &request)
{
    size_t start = request.find(" /r/");
    if (start == std::string::npos) return {};
    start += 4;
    size_t end = request.find_first_of(" ?", start);
    std::string token = request.substr(start, end == std::string::npos ? std::string::npos : end - start);
    std::lock_guard<std::mutex> lock(gResponsesLock);
    auto found = gResponses.find(token);
    if (found == gResponses.end()) return {};
    std::string response = std::move(found->second);
    gResponses.erase(found);
    return response;
}

void answer(int client)
{
    char buffer[4096];
    timeval timeout{1, 0};
    setsockopt(client, SOL_SOCKET, SO_RCVTIMEO, &timeout, sizeof(timeout));
    std::string request;
    ssize_t received = recv(client, buffer, sizeof(buffer), 0);
    if (received > 0) request.assign(buffer, static_cast<size_t>(received));
    while (recv(client, buffer, sizeof(buffer), MSG_DONTWAIT) > 0) {
    }
    std::string response = registeredResponse(request);
    if (response.empty())
        send(client, kEmptyResponse, sizeof(kEmptyResponse) - 1, 0);
    else
        send(client, response.data(), response.size(), 0);
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

std::string respondWith(int status, const std::vector<std::pair<std::string, std::string>> &headers, const std::string &body)
{
    std::string base = url();
    if (base.empty()) return {};
    std::string response =
        "HTTP/1.1 " + std::to_string(status) + " astral\r\nContent-Length: " + std::to_string(body.size()) + "\r\nConnection: close\r\n";
    bool typed = false;
    for (const auto &[name, value] : headers) {
        if (name == "content-length" || name == "connection" || name == "content-encoding") continue;
        typed |= name == "content-type";
        response += name + ": " + value + "\r\n";
    }
    if (!typed && !body.empty()) response += "content-type: application/json\r\n";
    response += "\r\n" + body;
    std::string token = std::to_string(gNextToken.fetch_add(1));
    std::lock_guard<std::mutex> lock(gResponsesLock);
    if (gResponses.size() > 64) gResponses.erase(gResponses.begin());
    gResponses[token] = std::move(response);
    return base + "r/" + token;
}

} // namespace astral::sink
